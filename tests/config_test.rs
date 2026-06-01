//! Unit tests for config module: BindAddr parsing.

use randimg_backend_rs::config::BindAddr;
use std::net::SocketAddr;

#[test]
fn test_parse_tcp_plain_address() {
    let addr = BindAddr::parse("127.0.0.1:8000");
    match addr {
        BindAddr::Tcp(sa) => assert_eq!(sa, "127.0.0.1:8000".parse::<SocketAddr>().unwrap()),
        _ => panic!("Expected Tcp"),
    }
}

#[test]
fn test_parse_tcp_with_http_scheme() {
    let addr = BindAddr::parse("http://127.0.0.1:3000");
    match addr {
        BindAddr::Tcp(sa) => assert_eq!(sa, "127.0.0.1:3000".parse::<SocketAddr>().unwrap()),
        _ => panic!("Expected Tcp"),
    }
}

#[test]
fn test_parse_tcp_with_https_scheme() {
    let addr = BindAddr::parse("https://0.0.0.0:443");
    match addr {
        BindAddr::Tcp(sa) => assert_eq!(sa, "0.0.0.0:443".parse::<SocketAddr>().unwrap()),
        _ => panic!("Expected Tcp"),
    }
}

#[test]
fn test_parse_unix_socket() {
    let addr = BindAddr::parse("unix:///run/randimg.sock");
    match addr {
        BindAddr::Unix(path) => assert_eq!(path.to_str().unwrap(), "/run/randimg.sock"),
        _ => panic!("Expected Unix"),
    }
}

#[test]
fn test_parse_tcp_wildcard() {
    let addr = BindAddr::parse("0.0.0.0:8080");
    match addr {
        BindAddr::Tcp(sa) => {
            assert_eq!(sa.ip(), "0.0.0.0".parse::<std::net::IpAddr>().unwrap());
            assert_eq!(sa.port(), 8080);
        }
        _ => panic!("Expected Tcp"),
    }
}

#[test]
fn test_parse_localhost_resolves() {
    // localhost:9999 should resolve via DNS
    let addr = BindAddr::parse("localhost:9999");
    match addr {
        BindAddr::Tcp(sa) => assert_eq!(sa.port(), 9999),
        _ => panic!("Expected Tcp"),
    }
}

#[test]
#[should_panic]
fn test_parse_invalid_address_panics() {
    BindAddr::parse("not-a-valid-address:xyz");
}

#[test]
fn test_bind_addr_is_clone_and_debug() {
    let addr = BindAddr::parse("127.0.0.1:8080");
    let cloned = addr.clone();
    let debug_str = format!("{:?}", cloned);
    assert!(debug_str.contains("Tcp"));
}
