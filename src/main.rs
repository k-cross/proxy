use clap::Parser;
use hyper::{Body, Client, Request, Response, Server, StatusCode, Method, Uri};
use hyper::service::service_fn;
use hyper::upgrade::Upgraded;
use std::net::SocketAddr;
use tokio::net::TcpStream;
use tower::make::Shared;

#[derive(Debug, Parser)]
#[clap(about, version, long_about = None)]
pub struct Opts {
    /// Which port to listen on
    #[clap(short, long, default_value_t = 7777)]
    port: u16,
}

#[tokio::main]
async fn main() {
    let opts = Opts::parse();
    let make_service = Shared::new(service_fn(log));

    let addr = SocketAddr::from(([127, 0, 0, 1], opts.port));
    let server = Server::bind(&addr).serve(make_service);
    println!("Listening at: http://{}", &addr);

    if let Err(e) = server.await {
        println!("error: {}", e);
    }
}

async fn log(mut req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
    dbg!(&req);
    dbg!(&req.uri());
    dbg!(&req.uri().authority());
    if req.uri().host().unwrap() == "api.formswift.com" {
        let p = req.uri().path();
        let q = match req.uri().query() {
            Some(query) => format!("{}&with_ecs=true", query),
            None => "with_ecs=true".to_string(),
        };

        let urib = Uri::builder()
            .scheme("https")
            .path_and_query(format!("{}?{}", p, q))
            .authority("api.formswift.com:443")
            .build()
            .unwrap();

        *req.uri_mut() = urib;
    }
    dbg!(&req.uri());

    if req.method() == Method::CONNECT {
        // Received an HTTP request like:
        // ```
        // CONNECT www.domain.com:443 HTTP/1.1
        // Host: www.domain.com:443
        // Proxy-Connection: Keep-Alive
        // ```
        //
        // When HTTP method is CONNECT return an empty body then upgrade the connection and talk a
        // new protocol.
        //
        // Note: only after client received an empty body with STATUS_OK can the connection be
        // upgraded, so we can't return a response inside `on_upgrade` future.
        if let Some(addr) = host_addr(req.uri()) {
            tokio::task::spawn(async move {
                match hyper::upgrade::on(req).await {
                    Ok(upgraded) => {
                        if let Err(e) = tunnel(upgraded, addr).await {
                            eprintln!("server io error: {}", e);
                        };
                    }
                    Err(e) => eprintln!("upgrade error: {}", e),
                }
            });

            Ok(Response::new(Body::empty()))
        } else {
            eprintln!("CONNECT host is not socket addr: {:?}", req.uri());
            let mut resp = Response::new(Body::from("CONNECT must be to a socket address"));
            *resp.status_mut() = StatusCode::BAD_REQUEST;

            Ok(resp)
        }
    } else {
        let path = req.uri().path();

        if path.starts_with("/api") {
            println!("API Path: {}", path);
        } else {
            println!("Generic Path: {}", path);
        }

        handle(req).await
    }
}

fn host_addr(uri: &Uri) -> Option<String> {
    uri.authority().and_then(|auth| Some(auth.to_string()))
}

// Create a TCP connection to host:port, build a tunnel between the connection and the upgraded
// connection
async fn tunnel(mut upgraded: Upgraded, addr: String) -> std::io::Result<()> {
    // Connect to remote server
    let mut server = TcpStream::connect(addr).await?;

    // Proxying data
    let (from_client, from_server) =
        tokio::io::copy_bidirectional(&mut upgraded, &mut server).await?;

    // Print message when done
    println!(
        "client wrote {} bytes and received {} bytes",
        from_client, from_server
    );

    Ok(())
}

async fn handle(req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
    let client = Client::new();
    client.request(req).await
}
