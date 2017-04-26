use error::{Error,Result};
use peer::{PeerID,Message};
use peer;
use metainfo::{MetaInfo};
use tracker::{TrackerClient};
use std::net::TcpStream;
use std::io::Write;

pub struct Downloader {
}

impl Downloader {
    pub fn start(info: MetaInfo, peer_id: PeerID) -> Result<()> {
        let mut tc = TrackerClient::new(info.clone(), peer_id)?;
        println!("trackerclient: {:#?}", tc);

        let tracker_res = tc.easy_start()?;
        println!("tracker res: {:#?}", tracker_res);
        if let Some(reason) = tracker_res.failure_reason {
            return Err(Error::new_str(&format!("tracker failed: {}", reason)));
        }

        if tracker_res.peers.is_empty() {
            return Err(Error::new_str("tracker returned no peers"));
        }

        let peer = tracker_res.peers[0].clone();
        let mut stream = TcpStream::connect(peer.address)?;
        peer::handshake_send(&mut stream, info.info_hash.clone(), peer_id)?;
        stream.flush()?;

        let remote_info_hash = peer::handshake_read_1(&mut stream)?;
        println!("remote info hash: {:?}", remote_info_hash);
        if remote_info_hash != info.info_hash {
            return Err(Error::new_str(&format!("peer info hash mismatch peer:{:?} me:{:?}",
                remote_info_hash, info.info_hash)));
        }
        let remote_peer_id = peer::handshake_read_2(&mut stream)?;
        println!("remote peer id: {:?}", remote_peer_id);

        loop {
            let m = peer::read_message(&mut stream)?;
            println!("message: {}", m.summarize());
            match m {
                Message::Bitfield { bits } => {
                    if bits.len() < info.num_pieces() {
                        println!("{}", Error::new_str(&format!("bitfield has less bits {} than pieces {}",
                                                           bits.len(), info.num_pieces())));
                    }
                },
                _ => {},
            }
        }
    }
}
