extern crate bip_bencode;
extern crate docopt;
extern crate hyper;
extern crate itertools;
extern crate ring;
extern crate rustc_serialize;
extern crate url;

mod metainfo;
mod tracker;

use bip_bencode::{BencodeRef, BDecodeOpt};
use docopt::Docopt;
use itertools::Itertools;
use metainfo::*;
use ring::rand::SystemRandom;
use std::error::Error;
use std::fs::File;
use std::io::prelude::*;
use std::path::Path;
use tracker::*;

const USAGE: &'static str = "
Usage: bittles <torrent>
";

#[derive(RustcDecodable)]
struct Args {
    arg_torrent: String,
}

fn main() {
    match inner2() {
        Ok(_) => {}
        Err(e) => println!("ERROR: {}", e)
    }
}

fn inner2() -> Result<(),Box<Error>> {
    let x: [u8; 20] = [18, 52, 86, 120, 154, 188, 222, 241, 35, 69, 103, 137, 171, 205, 239, 18, 52, 86, 120, 154];
    let y = format!("{:x}", x.iter().format(""));
    println!("{}", y);

    let z = url::percent_encoding::percent_encode(&x, url::percent_encoding::QUERY_ENCODE_SET).collect::<String>();
    println!("{}", z);

    assert_eq!("%124Vx%9A%BC%DE%F1%23Eg%89%AB%CD%EF%124Vx%9A", z);

    let mut url = hyper::Url::parse("http://example.com/announce")?;
    {
        let mut qp = url.query_pairs_mut();
        qp.clear();
        // qp.append_pair("info_hash", "asdlfkjskd\u{00cc}f");
        qp.append_pair("info_hash", &z);
    }

    println!("{:?}", url);

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

    let tc = TrackerClient::new(&rand, info.clone(), peer_id)?;
    println!("trackerclient: {:?}", tc);

    println!("tracker res: {:?}", tc.easy_start()?);

    Ok(())
}
