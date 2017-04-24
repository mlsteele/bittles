use error::{Error,Result};
use metainfo::{InfoHash};
use peer::{PeerID,handshake_send};
use peer;
use metainfo::{MetaInfo};
use tracker::{TrackerClient};
use std::net::TcpStream;

pub struct Downloader {
}

impl Downloader {
    pub fn start(info: MetaInfo, peer_id: PeerID) -> Result<()> {
        let mut tc = TrackerClient::new(info.clone(), peer_id)?;
        println!("trackerclient: {:?}", tc);

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
        peer::handshake_send(&mut stream, info.info_hash, peer_id)?;
        let remote_info_hash = peer::handshake_read_1(&mut stream)?;
        println!("remote info hash: {:?}", remote_info_hash);
        let remote_peer_id = peer::handshake_read_2(&mut stream)?;
        println!("remote peer id: {:?}", remote_peer_id);

        Ok(())
    }
}
