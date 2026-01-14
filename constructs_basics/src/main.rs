fn main() {
    // enums
    enum IpAddrKind {
        V4,
        V6,
    }
    let ipv4 = IpAddrKind::V4;

    fn route(ip_kind: IpAddrKind) {
        // ...
    }

    route(ipv4);

    struct IpAddr {
        kind: IpAddrKind,
        address: String,
    }

    let home = IpAddr {
        kind: IpAddrKind::V4,
        address: String::from("0.0.0.0"),
    };
}
