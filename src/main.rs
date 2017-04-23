extern crate bip_bencode;
extern crate byteorder;
extern crate docopt;
extern crate hyper;
extern crate itertools;
extern crate ring;
extern crate rustc_serialize;
extern crate url;

mod error;
mod metainfo;
mod tracker;
mod util;
mod peer;

use bip_bencode::{BencodeRef, BDecodeOpt};
use docopt::Docopt;
use itertools::Itertools;
use metainfo::*;
use peer::{new_peer_id};
use ring::rand::SystemRandom;
use std::error::Error;
use std::fs::File;
use std::io::prelude::*;
use std::path::Path;
use tracker::*;
use util::{replace_query_parameters, QueryParameters};

const USAGE: &'static str = "
Usage: bittles <torrent>
";

#[derive(RustcDecodable)]
struct Args {
    arg_torrent: String,
}

fn main() {
    match inner() {
        Ok(_) => {}
        Err(e) => println!("ERROR: {}", e)
    }
}

#[allow(dead_code)]
fn inner2() -> Result<(),Box<Error>> {
    let x: [u8; 20] = [18, 52, 86, 120, 154, 188, 222, 241, 35, 69, 103, 137, 171, 205, 239, 18, 52, 86, 120, 154];
    let y = format!("{:x}", x.iter().format(""));
    println!("hex: {}", y);

    let z = url::percent_encoding::percent_encode(&x, url::percent_encoding::QUERY_ENCODE_SET).collect::<String>();
    println!("url encoded: {}", z);

    assert_eq!("%124Vx%9A%BC%DE%F1%23Eg%89%AB%CD%EF%124Vx%9A", z);

    {
        let mut url = hyper::Url::parse("http://example.com/announce")?;
        {
            let mut qp = url.query_pairs_mut();
            qp.clear();
            // qp.append_pair("info_hash", "asdlfkjskd\u{00cc}f");
            qp.append_pair("info_hash", &z);
        }

        println!("url qpm: {:?}", url);
    }

    {
        let mut url = hyper::Url::parse("http://example.com/announce")?;
        url.set_query(Some(&format!("info_hash={}", z)));
        println!("url s-q: {:?}", url);
    }

    {
        let mut url = hyper::Url::parse("http://example.com/announce")?;
        replace_query_parameters(&mut url,
                                 &[("info_hash", x)]
        );
        println!("url uuz: {:?}", url);
    }

    {
        let mut url = hyper::Url::parse("http://example.com/announce")?;
        let mut qps = QueryParameters::new();
        qps.push("info_hash", x);
        qps.apply(&mut url);
        println!("url uub: {:?}", url);
    }

    Ok(())
}

#[allow(dead_code)]
fn inner3() -> Result<(),Box<Error>> {
    // ubuntu info hash
    let x: [u8; 20] = [52, 147, 6, 116, 239, 59, 185, 49, 127, 181, 242, 99, 204, 168, 48, 245, 38, 133, 35, 91];
    let y = format!("{:x}", x.iter().format(""));
    println!("hex: {}", y);

    let reference = "4%93%06t%EF%3B%B91%7F%B5%F2c%CC%A80%F5%26%85%23%5B";

    // let z = url::percent_encoding::percent_encode(&x, url::percent_encoding::QUERY_ENCODE_SET).collect::<String>();
    let z = url::form_urlencoded::byte_serialize(&x).collect::<String>();
    println!("        ref: {}", reference);
    println!("url encoded: {}", z);

    assert_eq!(reference, z);

    {
        let mut url = hyper::Url::parse("http://example.com/announce")?;
        let mut qps = QueryParameters::new();
        qps.push("info_hash", x);
        qps.apply(&mut url);
        println!("url uub: {:?}", url);
    }

    Ok(())
}

fn inner() -> Result<(),Box<Error>> {
    let args: Args = Docopt::new(USAGE)
        .and_then(|d| d.decode())
        .unwrap_or_else(|e| e.exit());

    let cwd = std::env::current_dir()?;
    println!("cwd: {}", cwd.display());
    println!("torrent: {}", args.arg_torrent);

    let torrent_path = Path::new(&args.arg_torrent);
    let mut buf = vec![0;0];
    let mut f = File::open(torrent_path)?;
    f.read_to_end(&mut buf).unwrap();

    let res = BencodeRef::decode(buf.as_slice(), BDecodeOpt::default())?;

    let info = MetaInfo::new(res)?;
    println!("{:?}", info);

    let rand = SystemRandom::new();
    let peer_id = new_peer_id(&rand)?;
    println!("peer_id: {:x}", peer_id.iter().format(""));

    let mut tc = TrackerClient::new(info.clone(), peer_id)?;
    println!("trackerclient: {:?}", tc);

    println!("tracker res: {:#?}", tc.easy_start()?);

    Ok(())
}
