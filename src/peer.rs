use byteorder::{ByteOrder,BigEndian};
use error::{Error,Result};
use metainfo::{InfoHash};
use ring::rand::SystemRandom;
use std::io::Read;
use std::io;
use std::net;

pub const PEERID_SIZE: usize = 20;
pub type PeerID = [u8; PEERID_SIZE];

pub fn new_peer_id(rand: &SystemRandom) -> Result<PeerID> {
    let mut id = [0; PEERID_SIZE];
    rand.fill(&mut id)?;
    id[0] = '-' as u8;
    id[1] = 'B' as u8;
    id[2] = 'I' as u8;
    id[3] = '0' as u8;
    id[4] = '0' as u8;
    id[5] = '0' as u8;
    id[6] = '1' as u8;
    Ok(id)
}

// Client to talk to a peer
#[derive(Debug)]
pub struct PeerClient {
    /// Address of the remote peer
    address: net::SocketAddr,
    /// Whether or not the remote peer is interested in something this client has to offer
    peer_interested: bool,
    peer_choking: bool,
    am_choking: bool,
    am_interested: bool,
}

fn read_message<T: io::Read>(stream: &mut T) -> Result<DataMessage> {
    let message_length = {
        let mut buf = [0; 4];
        stream.read_exact(&mut buf)?;
        BigEndian::read_u32(&buf)
    };
    let data: Vec<u8> = {
        let mut buf = Vec::new();
        let mut sub = stream.take(message_length as u64);
        sub.read_to_end(&mut buf)?;
        buf
    };
    Ok(DataMessage{ data: data })
}

#[cfg(test)]
mod tests {
    use peer::*;

    #[test]
    fn test_read_message() {
        let sample = vec![0, 0, 0, 3, 1, 2, 3, 0, 0, 0, 5, 11, 12, 13, 14, 15, 9, 9, 9, 9, 9];
        let mut reader = sample.as_slice();
        {
            let d = read_message(&mut reader).unwrap();
            assert_eq!(d.data, vec![1, 2, 3]);
        } {
            let d = read_message(&mut reader).unwrap();
            assert_eq!(d.data, vec![11, 12, 13, 14, 15]);
        }
    }
}

pub struct DataMessage {
    data: Vec<u8>,
}

// In version 1.0 of the BitTorrent protocol, pstrlen = 19, and pstr = "BitTorrent protocol".
const HANDSHAKE_PROTOCOL: &'static str = "BitTorrent protocol";

// trait Message {
//     fn encode(&self) -> Result<&[u8]>;
// }

pub enum Message {
    KeepAlive,
    Choke,
    Unchoke,
    Interested,
    NotInterested,
    Have,
    Bitfield,
    Request,
    Piece,
    Cancel,
    Port,
}

pub struct Handshake {
    info_hash: InfoHash,
    peer_id: PeerID,
}

pub struct KeepAlive {}

pub struct Choke {}

pub struct Unchoke {}

pub struct Interested {}

pub struct NotInterested {}

pub struct Have {
    /// Piece index
    piece: i64,
}

pub struct Bitfield {
    /// One bool per piece. True means the peer has the complete piece.
    bits: [bool],
}

pub struct Request {
    /// Piece index
    piece: i64, // (index)
    /// Offset within the piece
    offset: i64, // (begin)
    /// Length in bytes
    length: i64,
}

pub struct Piece<'a> {
    /// Piece index
    piece: i64, // (index)
    /// Offset within the piece
    offset: i64, // (begin)
    /// Block data
    block: &'a [u8],
}

pub struct Cancel {
    /// Piece index
    piece: i64, // (index)
    /// Offset within the piece
    offset: i64, // (begin)
    /// Length in bytes
    length: i64,
}

pub struct Port {
    /// The listen port is the port this peer's DHT node is listening on.
    port: i64,
}
