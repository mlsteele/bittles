use byteorder::{ByteOrder,BigEndian};
use error::{Error,Result};
use metainfo::{InfoHash};
use ring::rand::SystemRandom;
use std::io::Read;
use std::io;
use std::net;
use std::cmp;
use util::ReadWire;

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

fn read_message<T: io::Read>(stream: &mut T) -> Result<Message> {
    use peer::Message::*;
    // Message length including ID. Not including its own 4 bytes.
    let message_length = stream.read_u32()?;
    if message_length == 0 {
        return Ok(KeepAlive);
    }
    if message_length < 4 {
        return Err(Error::new_str(&format!("message length ({}) less than 4", message_length)));
    }
    let message_id = {
        let mut buf = [0; 4];
        stream.read_exact(&mut buf)?;
        BigEndian::read_u32(&buf)
    };
    let data: Vec<u8> = {
        let data_length = cmp::max(0, message_length - 4); // subtract 4 for the message_id
        if data_length <= 0 {
            Vec::new()
        } else {
            let mut buf = Vec::new();
            let mut sub = stream.take(data_length as u64);
            sub.read_to_end(&mut buf)?;
            buf
        }
    };
    match message_id {
        0 => Ok(Choke),
        1 => Ok(Unchoke),
        2 => Ok(Interested),
        3 => Ok(NotInterested),
        // 4 => Ok(Have),
        // 5 => Ok(Bitfield),
        // 6 => Ok(Request),
        // 7 => Ok(Piece),
        // 8 => Ok(Cancel),
        // 9 => Ok(Port),
        _ => Err(Error::new_str(&format!("unrecognized message id {}", message_id))),
    }
}

#[cfg(test)]
mod tests {
    use peer::*;

    #[test]
    fn test_read_message() {
        let sample = vec![0, 0, 0, 4, 0, 0, 0, 2,
                          0, 0, 0, 4, 0, 0, 0, 1,
                          9, 9];
        let mut reader = sample.as_slice();
        {
            let m = read_message(&mut reader).unwrap();
            assert!(matches!(m, Message::Interested));
        }
        {
            let m = read_message(&mut reader).unwrap();
            assert!(matches!(m, Message::Unchoke));
        }
    }
}

// In version 1.0 of the BitTorrent protocol, pstrlen = 19, and pstr = "BitTorrent protocol".
const HANDSHAKE_PROTOCOL: &'static str = "BitTorrent protocol";

pub enum Message {
    KeepAlive,
    Choke,
    Unchoke,
    Interested,
    NotInterested,
    Have {
        /// Piece index
        piece: i64,
    },
    Bitfield {
        /// One bool per piece. True means the peer has the complete piece.
        bits: Box<[bool]>,
    },
    Request {
        /// Piece index
        piece: i64, // (index)
        /// Offset within the piece
        offset: i64, // (begin)
        /// Length in bytes
        length: i64,
    },
    Piece {
        /// Piece index
        piece: i64, // (index)
        /// Offset within the piece
        offset: i64, // (begin)
        /// Block data
        block: Box<[u8]>,
    },
    Cancel {
        /// Piece index
        piece: i64, // (index)
        /// Offset within the piece
        offset: i64, // (begin)
        /// Length in bytes
        length: i64,
    },
    Port {
        /// The listen port is the port this peer's DHT node is listening on.
        port: i64,
    },
}

pub struct Handshake {
    info_hash: InfoHash,
    peer_id: PeerID,
}
