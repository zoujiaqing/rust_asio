use std::io;
use std::fmt;
use std::sync::Arc;
use {IoObject, Strand, Protocol, Endpoint, DgramSocket};
use ip::{IpEndpoint, Resolver, ResolverQuery, Passive, ResolverIter, UnsafeResolverIter, host_not_found};
use ops;
use ops::{AF_UNSPEC, AF_INET, AF_INET6, SOCK_DGRAM, AI_PASSIVE, AI_NUMERICHOST, AI_NUMERICSERV};

/// The User Datagram Protocol.
///
/// # Examples
/// In this example, Creates a UDP server socket and UDP client socket with resolving.
///
/// ```
/// use std::io;
/// use asio::{IoService, Protocol, Endpoint};
/// use asio::ip::{Udp, UdpSocket, UdpEndpoint, UdpResolver, ResolverIter, Passive};
///
/// fn udp_bind(io: &IoService, it: ResolverIter<Udp>) -> io::Result<UdpSocket> {
///     for e in it {
///         let ep = e.endpoint();
///         println!("{:?}", ep);
///         if let Ok(soc) = UdpSocket::new(io, ep.protocol()) {
///             if let Ok(_) = soc.bind(&ep) {
///                 return Ok(soc)
///             }
///         }
///     }
///     Err(io::Error::new(io::ErrorKind::Other, "Failed to bind"))
/// }
///
/// let io = IoService::new();
/// let re = UdpResolver::new(&io);
/// let sv = re.resolve((Passive, 12345))
///            .and_then(|it| udp_bind(&io, it))
///            .unwrap();
/// let cl = re.connect(("localhost", "12345"))
///            .unwrap();
/// ```
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct Udp {
    family: i32,
}

impl Udp {
    /// Represents a UDP for IPv4.
    pub fn v4() -> Udp {
        Udp { family: AF_INET as i32 }
    }

    /// Represents a UDP for IPv6.
    pub fn v6() -> Udp {
        Udp { family: AF_INET6 as i32 }
    }
}

impl Protocol for Udp {
    type Endpoint = IpEndpoint<Udp>;

    fn family_type(&self) -> i32 {
        self.family
    }

    fn socket_type(&self) -> i32 {
        SOCK_DGRAM as i32
    }

    fn protocol_type(&self) -> i32 {
        0
    }
}

impl Endpoint<Udp> for IpEndpoint<Udp> {
    fn protocol(&self) -> Udp {
        if self.is_v4() {
            Udp::v4()
        } else if self.is_v6() {
            Udp::v6()
        } else {
            unreachable!("Invalid domain ({}).", self.ss.ss_family);
        }
    }
}

impl DgramSocket<Udp> {
    /// Constructs a UDP socket.
    ///
    /// # Examples
    /// ```
    /// use asio::IoService;
    /// use asio::ip::{Udp, UdpSocket};
    ///
    /// let io = IoService::new();
    /// let udp4 = UdpSocket::new(&io, Udp::v4()).unwrap();
    /// let udp6 = UdpSocket::new(&io, Udp::v6()).unwrap();
    /// ```
    pub fn new<T: IoObject>(io: &T, pro: Udp) -> io::Result<DgramSocket<Udp>> {
        Ok(Self::_new(io, try!(ops::socket(pro))))
    }
}

impl fmt::Debug for DgramSocket<Udp> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "UdpSocket")
    }
}

impl Resolver<Udp> {
    pub fn connect<'a, Q: ResolverQuery<'a, Udp>>(&self, query: Q) -> io::Result<(UdpSocket, UdpEndpoint)> {
        let it = try!(query.iter());
        let mut err = host_not_found();
        for e in it {
            let ep = e.endpoint();
            let soc = try!(UdpSocket::new(self, ep.protocol()));
            match soc.connect(&ep) {
                Ok(_) => return Ok((soc, ep)),
                Err(e) => err = e,
            }
        }
        Err(err)
    }

    fn async_connect_impl<F, T>(&self, mut it: UnsafeResolverIter<Udp>, callback: F, strand: &Strand<T>)
        where F: FnOnce(Strand<T>, io::Result<(UdpSocket, UdpEndpoint)>) + Send + 'static,
              T: 'static {
        if let Some(e) = it.next() {
            let ep = e.endpoint();
            match UdpSocket::new(self, ep.protocol()) {
                Ok(soc) => {
                    let obj = Strand::new(self, (self as *const Self, it, soc, ep));
                    let obj_ = obj.obj.clone();
                    obj.2.async_connect(&obj.3, move |strand, res| {
                        let (re, it, soc, ep) = unsafe { Arc::try_unwrap(obj_).unwrap().into_inner() };
                        match res {
                            Ok(_) => {
                                callback(strand, Ok((soc, ep)));
                            },
                            Err(_) => {
                                unsafe { &*re }.async_connect_impl(it, callback, &strand);
                            },
                        }
                    }, strand);
                },
                Err(err) => {
                    self.io_service().post_strand(move |strand| callback(strand, Err(err)), strand);
                },
            }
        } else {
            let err = host_not_found();
            self.io_service().post_strand(move |strand| callback(strand, Err(err)), strand);
        }
    }

    pub fn async_connect<'a, Q, F, T>(&self, query: Q, callback: F, strand: &Strand<T>)
        where Q: ResolverQuery<'a, Udp>,
              F: FnOnce(Strand<T>, io::Result<(UdpSocket, UdpEndpoint)>) + Send + 'static,
              T: 'static {
        match query.iter() {
            Ok(it) => self.async_connect_impl(unsafe { it.into_inner() }, callback, strand),
            Err(err) => self.io_service().post_strand(move |strand| callback(strand, Err(err)), strand),
        }
    }
}

impl<'a> ResolverQuery<'a, Udp> for (Passive, u16) {
    fn iter(self) -> io::Result<ResolverIter<'a, Udp>> {
        let port = self.1.to_string();
        ResolverIter::_new(Udp { family: AF_UNSPEC }, "", &port, AI_PASSIVE | AI_NUMERICSERV)
    }
}

impl<'a, 'b> ResolverQuery<'a, Udp> for (Passive, &'b str) {
    fn iter(self) -> io::Result<ResolverIter<'a, Udp>> {
        ResolverIter::_new(Udp { family: AF_UNSPEC }, "", self.1, AI_PASSIVE)
    }
}

impl<'a, 'b, 'c> ResolverQuery<'a, Udp> for (&'b str, &'c str) {
    fn iter(self) -> io::Result<ResolverIter<'a, Udp>> {
        ResolverIter::_new(Udp { family: AF_UNSPEC }, self.0, self.1, 0)
    }
}

/// The UDP endpoint type.
pub type UdpEndpoint = IpEndpoint<Udp>;

/// The UDP socket type.
pub type UdpSocket = DgramSocket<Udp>;

/// The UDP resolver type.
pub type UdpResolver = Resolver<Udp>;

#[test]
fn test_udp() {
    assert!(Udp::v4() == Udp::v4());
    assert!(Udp::v6() == Udp::v6());
    assert!(Udp::v4() != Udp::v6());
}

#[test]
fn test_udp_resolve() {
    use IoService;
    use super::IpAddrV4;

    let io = IoService::new();
    let re = UdpResolver::new(&io);
    for e in re.resolve(("127.0.0.1", "80")).unwrap() {
        assert!(e.endpoint() == UdpEndpoint::new(IpAddrV4::new(127,0,0,1), 80));
    }
}