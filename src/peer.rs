use error::{Error,Result};
use metainfo::{InfoHash};
use ring::rand::SystemRandom;
use std::io::Read;
use std::io;
use std::net;
use util::{ReadWire,byte_to_bits};
use metainfo::{INFO_HASH_SIZE};

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

// In version 1.0 of the BitTorrent protocol, pstrlen = 19, and pstr = "BitTorrent protocol".
const HANDSHAKE_PROTOCOL: &'static str = "BitTorrent protocol";

/// Read the first half of the peer handshake.
pub fn handshake_read_1<T: io::Read>(stream: &mut T) -> Result<InfoHash> {
    // Protocol string
    let pstrlen = stream.read_u8()?;
    let pstr = stream.read_n(pstrlen as u64)?;
    if pstr != HANDSHAKE_PROTOCOL.as_bytes() {
        return Err(Error::new_peer(&format!("unrecognized protocol in handshake: {:?}", pstr)));
    }

    // 8 reserved bytes.
    let _ = stream.read_n(8)?;

    // Info hash
    let mut info_hash = [0; INFO_HASH_SIZE];
    stream.read_exact(&mut info_hash)?;
    Ok(InfoHash { hash: info_hash })
}

/// Read the last half of the peer handshake.
pub fn handshake_read_2<T: io::Read>(stream: &mut T) -> Result<PeerID> {
    // Peer id
    let mut peer_id: PeerID = [0; PEERID_SIZE];
    stream.read_exact(&mut peer_id)?;
    Ok(peer_id)
}

/// Sends one side of the peer handshake.
/// Used both for starting and handling connections.
/// Does not flush the stream.
pub fn handshake_send<T: io::Write>(stream: &mut T, info_hash: InfoHash, peer_id: PeerID) -> Result<()> {
    // Protocol string
    let pstr = HANDSHAKE_PROTOCOL.as_bytes();
    let pstrlen: u8 = pstr.len() as u8;
    stream.write(&[pstrlen])?;
    stream.write(pstr)?;

    // 8 reserved bytes
    stream.write(&[0; 8])?;

    // Info hash
    stream.write(&info_hash.hash)?;

    // Peer id
    stream.write(&peer_id)?;

    Ok(())
}

pub fn read_message<T: io::Read>(stream: &mut T) -> Result<Message> {
    use peer::Message::*;
    const NUM_LEN: usize = 4;
    // Message length including ID. Not including its own 4 bytes.
    let message_length: usize = stream.read_u32()? as usize;
    if message_length == 0 {
        return Ok(KeepAlive);
    }
    let message_id = stream.read_u8()?;
    println!("message id: {:?}", message_id);
    println!("message length: {:?}", message_length);
    if message_length == 1 {
        return match message_id {
            0 => Ok(Choke),
            1 => Ok(Unchoke),
            2 => Ok(Interested),
            3 => Ok(NotInterested),
            4 | 5 | 6 | 7 | 8 | 9 =>
                Err(Error::new_peer(&format!("message id {} specified no body", message_id))),
            _ => Err(Error::new_peer(&format!("unknown message id {} (no-body)", message_id))),
        }
    }
    let body_length: usize = message_length - 1;
    // Limit the stream to read to the end of this message.
    let mut stream = stream.take(body_length as u64);
    match message_id {
        0 | 1 | 2 | 3 =>
            Err(Error::new_peer(&format!("message id {} specified non-zero body {}", message_id, body_length))),
        4 => {
            if body_length != NUM_LEN {
                return Err(Error::new_peer(&format!("message wrong size 'Have' {} != 4", body_length)));
            }
            Ok(Have {
                piece: stream.read_u32()?
            })
        },
        5 => {
            let mut bits: Vec<bool> = Vec::new();
            for byte in stream.read_n(body_length as u64)? {
                bits.extend_from_slice(&byte_to_bits(byte));
            }
            Ok(Bitfield {
                bits: bits,
            })
        },
        6 => {
            if body_length != NUM_LEN * 3 {
                return Err(Error::new_peer(&format!("message wrong size 'Request' {} != 12", body_length)));
            }
            Ok(Request {
                piece: stream.read_u32()?,
                offset: stream.read_u32()?,
                length: stream.read_u32()?,
            })
        },
        7 => {
            if body_length < NUM_LEN * 2 {
                return Err(Error::new_peer(&format!("message wrong size 'Piece' {} < 8", body_length)));
            }
            let piece = stream.read_u32()?;
            let offset = stream.read_u32()?;
            let mut block = Vec::new();
            stream.read_to_end(&mut block)?;
            Ok(Piece {
                piece: piece,
                offset: offset,
                block: Box::new(block),
            })
        },
        8 => {
            if body_length != NUM_LEN * 3 {
                return Err(Error::new_peer(&format!("message wrong size 'Cancel' {} != 12", body_length)));
            }
            Ok(Cancel {
                piece: stream.read_u32()?,
                offset: stream.read_u32()?,
                length: stream.read_u32()?,
            })
        },
        9 => {
            if body_length != NUM_LEN {
                return Err(Error::new_peer(&format!("message wrong size 'Port' {} != 4", body_length)));
            }
            Ok(Port {
                port: stream.read_u32()?,
            })
        },
        _ => Err(Error::new_peer(&format!("unknown message id {} (+body)", message_id))),
    }
}

#[cfg(test)]
mod tests {
    use peer::*;

    #[test]
    fn test_read_message() {
        let sample = vec![0, 0, 0, 1, 2,
                          0, 0, 0, 5, 4, 0, 1, 0, 1,
                          0, 0, 0, 1, 1,
                          9, 9];
        let mut reader = sample.as_slice();
        {
            let m = read_message(&mut reader).unwrap();
            println!("{:?}", m);
            assert!(matches!(m, Message::Interested));
        }
        {
            let m = read_message(&mut reader).unwrap();
            println!("{:?}", m);
            match m {
                Message::Have { piece } => assert_eq!(piece, 65537),
                _ => assert!(false)
            }
        }
        {
            let m = read_message(&mut reader).unwrap();
            println!("{:?}", m);
            assert!(matches!(m, Message::Unchoke));
        }
    }

    // #[test]
    fn test_reader() {
        // This test is to understand how Read.take works when you don't use it all.
        let sample = vec![0, 1, 2, 3, 4, 5, 6, 7, 8];
        let mut reader = sample.as_slice();
        let mut reader2 = reader.take(1);
        assert_eq!(reader2.read_n(3).unwrap(), vec![0, 1, 2]);
    }
}

pub struct HandshakeResult {
    info_hash: InfoHash,
    peer_id: PeerID,
}

#[derive(Debug)]
pub enum Message {
    KeepAlive,
    Choke,
    Unchoke,
    Interested,
    NotInterested,
    Have {
        /// Piece index
        piece: u32,
    },
    Bitfield {
        /// One bool per piece. True means the peer has the complete piece.
        /// There may be trailing false's.
        bits: Vec<bool>,
    },
    Request {
        /// Piece index
        piece: u32, // (index)
        /// Offset within the piece
        offset: u32, // (begin)
        /// Length in bytes
        length: u32,
    },
    Piece {
        /// Piece index
        piece: u32, // (index)
        /// Offset within the piece
        offset: u32, // (begin)
        /// Block data
        block: Box<Vec<u8>>,
    },
    Cancel {
        /// Piece index
        piece: u32, // (index)
        /// Offset within the piece
        offset: u32, // (begin)
        /// Length in bytes
        length: u32,
    },
    Port {
        /// The listen port is the port this peer's DHT node is listening on.
        port: u32,
    },
}

impl Message {
    pub fn summarize(&self) -> String {
        match self {
            &Message::Bitfield { ref bits } => {
                let total = bits.len();
                let set = bits.iter().filter(|b| **b).count();
                format!("Bitfield {{ total:{} set:{} }}", total, set)
            },
            _ => format!("{:?}", self),
        }
    }
}

