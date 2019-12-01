#[macro_use]
extern crate log;

use cabot;

use std::collections::HashMap;
use std::iter::FromIterator;
use std::net::{AddrParseError, SocketAddr};
use std::pin::Pin;

use async_std;
use async_std::fs::OpenOptions;
use async_std::io::{self, Write};
use async_std::prelude::*;
use async_std::task::Context;
use async_std::task::Poll;
use clap::{App, Arg};
use log::Level::Info;

use cabot::constants;
use cabot::http;
use cabot::request::RequestBuilder;
use cabot::results::CabotResult;

pub async fn run() -> CabotResult<()> {
    let user_agent: String = constants::user_agent();
    let matches = App::new(env!("CARGO_PKG_NAME"))
        .version(constants::VERSION)
        .author(env!("CARGO_PKG_AUTHORS"))
        .about(env!("CARGO_PKG_DESCRIPTION"))
        .arg(
            Arg::with_name("URL")
                .index(1)
                .required(true)
                .help("URL to request"),
        )
        .arg(
            Arg::with_name("REQUEST")
                .short("X")
                .long("request")
                .default_value("GET")
                .help("Specify request command to use"),
        )
        .arg(
            Arg::with_name("HEADER")
                .short("H")
                .long("header")
                .takes_value(true)
                .multiple(true)
                .help("Pass custom header to server"),
        )
        .arg(
            Arg::with_name("FILE")
                .short("o")
                .long("output")
                .takes_value(true)
                .help("Write to FILE instead of stdout"),
        )
        .arg(
            Arg::with_name("VERBOSE")
                .short("v")
                .long("verbose")
                .help("Make the operation more talkative"),
        )
        .arg(
            Arg::with_name("BODY")
                .short("d")
                .long("data")
                .takes_value(true)
                .help("Post Data (Using utf-8 encoding)"),
        )
        .arg(
            Arg::with_name("IPv4")
                .short("4")
                .long("ipv4")
                .help("Resolve host names to IPv4 addresses"),
        )
        .arg(
            Arg::with_name("IPv6")
                .short("6")
                .long("ipv6")
                .help("Resolve host names to IPv6 addresses"),
        )
        .arg(
            Arg::with_name("UA")
                .short("A")
                .long("user-agent")
                .default_value(user_agent.as_str())
                .help("The user-agent HTTP header to use"),
        )
        .arg(
            Arg::with_name("RESOLVE")
                .long("resolve")
                .takes_value(true)
                .multiple(true)
                .help("<host:port:address> Resolve the host+port to this address"),
        )
        .get_matches();

    let url = matches.value_of("URL").unwrap();
    let http_method = matches.value_of("REQUEST").unwrap();
    let verbose = matches.is_present("VERBOSE");
    let body = matches.value_of("BODY");
    let ua = matches.value_of("UA").unwrap();

    let mut ipv4 = matches.is_present("IPv4");
    let mut ipv6 = matches.is_present("IPv6");

    if !ipv4 && !ipv6 {
        ipv4 = true;
        ipv6 = true;
    }

    let headers: Vec<&str> = match matches.values_of("HEADER") {
        Some(headers) => headers.collect(),
        None => Vec::new(),
    };

    let resolved: HashMap<String, SocketAddr> = match matches.values_of("RESOLVE") {
        Some(headers) => HashMap::from_iter(
            headers
                .map(|resolv| resolv.splitn(3, ':'))
                .map(|resolv| {
                    let count = resolv.clone().count();
                    if count != 3 {
                        let resolv: Vec<&str> = resolv.collect();
                        let _ =
                            eprintln!("Invalid format in resolve argument: {}", resolv.join(":"));
                        std::process::exit(1);
                    }
                    resolv
                })
                .map(|mut resolv| {
                    (
                        resolv.next().unwrap(),
                        resolv.next().unwrap(),
                        resolv.next().unwrap(),
                    )
                })
                .map(|(host, port, addr)| {
                    let parsed_port = port.parse::<usize>();
                    if parsed_port.is_err() {
                        let _ = eprintln!(
                            "Invalid port in resolve argument: {}:{}:{}",
                            host, port, addr
                        );
                        std::process::exit(1);
                    }
                    let sockaddr: Result<SocketAddr, AddrParseError> =
                        format!("{}:{}", addr, port).parse();
                    if sockaddr.is_err() {
                        let _ = eprintln!(
                            "Invalid address in resolve argument: {}:{}:{}",
                            host, port, addr
                        );
                        std::process::exit(1);
                    }
                    let sockaddr = sockaddr.unwrap();
                    (format!("{}:{}", host, port), sockaddr)
                }),
        ),
        None => HashMap::new(),
    };

    let mut builder = RequestBuilder::new(url)
        .set_http_method(http_method)
        .set_user_agent(ua)
        .add_headers(&headers.as_slice());

    if body.is_some() {
        builder = builder.set_body_as_str(body.unwrap());
    }

    let request = builder.build()?;

    if let Some(path) = matches.value_of("FILE") {
        let mut f = OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .open(path)
            .await
            .unwrap();
        http::http_query(
            &request,
            &mut CabotBinWrite::new(&mut f, verbose),
            &resolved,
            verbose,
            ipv4,
            ipv6,
        )
        .await?
    } else {
        http::http_query(
            &request,
            &mut CabotBinWrite::new(&mut io::stdout(), verbose),
            &resolved,
            verbose,
            ipv4,
            ipv6,
        )
        .await?
    };
    Ok(())
}

#[async_std::main]
async fn main() {
    pretty_env_logger::init();
    debug!("Starting cabot");

    run()
        .await
        .map(|ok| {
            debug!("Command cabot ended succesfully");
            ok
        })
        .map_err(|err| {
            eprintln!("{}", err);
            std::process::exit(1);
        })
        .unwrap();
}

// Internal Of the Binary

struct CabotBinWrite<'a> {
    out: &'a mut (dyn Write + Unpin),
    header_read: bool,
    verbose: bool,
}

impl<'a> CabotBinWrite<'a> {
    pub fn new(out: &'a mut (dyn Write + Unpin), verbose: bool) -> Self {
        CabotBinWrite {
            out,
            verbose,
            header_read: false,
        }
    }
    fn display_headers(&self, buf: &[u8]) {
        let headers = String::from_utf8_lossy(buf);
        let split: Vec<&str> = constants::SPLIT_HEADER_RE.split(&headers).collect();
        if log_enabled!(Info) {
            for part in split {
                info!("< {}", part);
            }
        } else if self.verbose {
            for part in split {
                eprintln!("< {}", part);
            }
        }
    }
}

impl<'a> Write for CabotBinWrite<'a> {
    // may receive headers
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context, buf: &[u8]) -> Poll<io::Result<usize>> {
        let self_ = Pin::get_mut(self);
        if !self_.header_read {
            // the first time write is called, all headers are sent
            // in the buffer. there is no need to parse it again.
            if log_enabled!(Info) || self_.verbose {
                self_.display_headers(&buf);
            }
            self_.header_read = true;
            Poll::Ready(Ok(0))
        } else {
            Pin::new(&mut self_.out).poll_write(cx, buf)
        }
    }

    /// this function is called when the request is done
    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context) -> Poll<io::Result<()>> {
        let self_ = Pin::get_mut(self);
        self_.out.flush();
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, _cx: &mut Context) -> Poll<io::Result<()>> {
        // let self_ = Pin::get_mut(self);
        // self_.out.close();
        Poll::Ready(Ok(()))
    }
}
