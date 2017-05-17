use byteorder::{ByteOrder, BigEndian};
use ring::rand::SystemRandom;
use std::fmt;
use std;
use bytes::{BytesMut, Buf};
use tokio_io;
use tokio_io::{AsyncWrite, AsyncRead};
use futures::future::{Future, BoxFuture};
use futures::future;

use errors::{Error, Result};
use itertools::Itertools;
use metainfo::INFO_HASH_SIZE;
use metainfo::InfoHash;
use util::byte_to_bits;

pub const PEERID_SIZE: usize = 20;
#[derive(Clone)]
pub struct PeerID {
    pub id: [u8; PEERID_SIZE],
}

impl fmt::Debug for PeerID {
    fn fmt(&self, f: &mut fmt::Formatter) -> std::result::Result<(), fmt::Error> {
        write!(f, "{:x}", self.id.iter().format(""))
    }
}

impl PeerID {
    pub fn new(rand: &SystemRandom) -> Result<Self> {
        let mut id = [0; PEERID_SIZE];
        rand.fill(&mut id)?;
        id[0] = '-' as u8;
        id[1] = 'B' as u8;
        id[2] = 'I' as u8;
        id[3] = '0' as u8;
        id[4] = '0' as u8;
        id[5] = '0' as u8;
        id[6] = '1' as u8;
        Ok(Self { id: id })
    }
}

// In version 1.0 of the BitTorrent protocol, pstrlen = 19, and pstr = "BitTorrent protocol".
const HANDSHAKE_PROTOCOL: &'static str = "BitTorrent protocol";

pub struct BitTorrentPeerCodec;

impl BitTorrentPeerCodec {
    // Sense whether there's a complete frame.
    // Does not take any bytes from src.
    // Returns Some(message_length) is there is a complete frame.
    // message_length does not include its own 4 bytes.
    // For example, for an `Interested`, message_length would be 1.
    // Returns None if not enough data to read a frame.
    fn sense_frame(src: &BytesMut) -> Option<u32> {
        const NUM_LEN: usize = 4;
        if src.len() < NUM_LEN {
            // Wait for the frame size
            return None;
        }
        let message_length = BigEndian::read_u32(src.as_ref());
        if src.len() + NUM_LEN < message_length as usize {
            // Wait for complete frame
            return None;
        }
        Some(message_length)
    }

    // Decode a single message.
    // Called with the exact frame (excluding message length tag).
    fn decode_message<B: Buf>(mut src: B) -> std::result::Result<Message, Error> {
        type BE = BigEndian;
        const NUM_LEN: usize = 4;
        use peer_protocol::Message::*;

        let message_length = src.remaining();

        if message_length == 0 {
            return Ok(KeepAlive);
        }
        let message_id = src.get_u8();

        if message_length == 1 {
            return match message_id {
                       0 => Ok(Choke),
                       1 => Ok(Unchoke),
                       2 => Ok(Interested),
                       3 => Ok(NotInterested),
                       4 | 5 | 6 | 7 | 8 | 9 => bail!("message id {} specified no body", message_id),
                       _ => bail!("unknown message id {} (no-body)", message_id),
                   };
        }

        let body_length: usize = message_length - 1;

        match message_id {
            0 | 1 | 2 | 3 => // Choke; Unchoke; Interested; NotInterested
                bail!("message id {} specified non-zero body {}", message_id, body_length),
            4 => { // Message::Have
                if body_length != NUM_LEN {
                    bail!("message wrong size 'Have' {} != 4", body_length);
                }
                Ok(Have {
                    piece: src.get_u32::<BE>()
                })
            },
            5 => { // Message::Bitfield
                let mut bits: Vec<bool> = Vec::new();
                for byte in src.iter() {
                    bits.extend_from_slice(&byte_to_bits(byte));
                }
                Ok(Bitfield {
                    bits: bits,
                })
            },
            6 => { // Message::Request
                if body_length != NUM_LEN * 3 {
                    bail!("message wrong size 'Request' {} != 12", body_length);
                }
                Ok(Request {
                    piece: src.get_u32::<BE>(),
                    offset: src.get_u32::<BE>(),
                    length: src.get_u32::<BE>(),
                })
            },
            7 => { // Message::Piece
                if body_length < NUM_LEN * 2 {
                    bail!("message wrong size 'Piece' {} < 8", body_length);
                }
                let piece = src.get_u32::<BE>();
                let offset = src.get_u32::<BE>();
                let block = src.bytes().to_vec();
                Ok(Piece {
                    piece: piece,
                    offset: offset,
                    block: block,
                })
            },
            8 => { // Message::Cancel
                if body_length != NUM_LEN * 3 {
                    bail!("message wrong size 'Cancel' {} != 12", body_length);
                }
                Ok(Cancel {
                    piece: src.get_u32::<BE>(),
                    offset: src.get_u32::<BE>(),
                    length: src.get_u32::<BE>(),
                })
            },
            9 => { // Message::Port
                if body_length != NUM_LEN {
                    bail!("message wrong size 'Port' {} != 4", body_length);
                }
                Ok(Port {
                    port: src.get_u32::<BE>(),
                })
            },
            _ => bail!("unknown message id {} (+body)", message_id),
        }
    }
}

impl tokio_io::codec::Decoder for BitTorrentPeerCodec {
    type Item = Message;
    type Error = Error;

    fn decode(&mut self, src: &mut BytesMut) -> std::result::Result<Option<Self::Item>, Self::Error> {
        const NUM_LEN: usize = 4;
        use bytes::IntoBuf;

        let message_length = match Self::sense_frame(&src) {
            Some(x) => x,
            None => return Ok(None),
        };

        // Drop the bytes representing message_length
        let _ = src.split_to(NUM_LEN);

        assert!(src.len() >= message_length as usize,
                "decoder bug: buf_len:{} < msg_len:{}",
                src.len(),
                message_length);

        // Take the message out of the source.
        // Also convert it to a Buf.
        // And replace the `src` name to confuse myself.
        let src = src.split_to(message_length as usize).freeze().into_buf();

        Self::decode_message(src).map(Some)
    }
}

impl tokio_io::codec::Encoder for BitTorrentPeerCodec {
    type Item = Message;
    type Error = Error;

    fn encode(&mut self, msg: Self::Item, dst: &mut BytesMut) -> std::result::Result<(), Self::Error> {
        type BE = BigEndian;
        use bytes::BufMut;
        use util::BytesMutEnhanced;
        let message_id = msg.message_id();
        match msg {
            Message::KeepAlive => {
                dst.ensure(4);
                dst.put_u32::<BE>(0); // message length
            }
            Message::Choke | Message::Unchoke | Message::Interested | Message::NotInterested => {
                dst.ensure(4 + 1);
                dst.put_u32::<BE>(1); // message length
                dst.put_u8(message_id);
            }
            Message::Request {
                piece,
                offset,
                length,
            } => {
                dst.ensure(4 + 1 + 4 * 3);
                dst.put_u32::<BE>(1 + 4 * 3); // message length
                dst.put_u8(message_id);
                dst.put_u32::<BE>(piece);
                dst.put_u32::<BE>(offset);
                dst.put_u32::<BE>(length);
            }
            Message::Bitfield { ref bits } => {
                // TODO(jessk) why is `ref` required here?
                // TODO(jessk) make this work
                // let msg: [u8] = bits.chunks(8).map(|bb| bits_to_byte(bb)).collect();
                // send_message_frame(stream, message_id, &msg)?;
                let _ = bits;
                bail!("send_message not implemented for id {}", message_id);
            }
            _ => {
                bail!("send_message not implemented for id {}", message_id);
            }
        }
        Ok(())
    }
}

/// Read the first half of the peer handshake.
pub fn handshake_read_1_async<R>(stream: R) -> BoxFuture<(R, InfoHash), Error>
    where R: AsyncRead + Send + 'static
{
    use tokio_io::io::read_exact;

    future::ok::<_, std::io::Error>(())
        // Protocol string
        .and_then(|_| {
            read_exact(stream, [0; 1])
        })
        .and_then(|(stream, buf_pstrlen)| {
            let pstrlen: u8 = buf_pstrlen[0];

            let buf_pstr = vec![0; pstrlen as usize];

            read_exact(stream, buf_pstr)
        })
        .and_then(|(stream, pstr)| {
            if pstr != HANDSHAKE_PROTOCOL.as_bytes() {
                let err_str = format!("unrecognized protocol in handshake: {:?}", pstr);
                let err2 = std::io::Error::new(std::io::ErrorKind::InvalidData, err_str);
                return Err(err2)
            }
            Ok(stream)
        })
        .and_then(|stream| {
            // 8 reserved bytes.
            read_exact(stream, [0; 8])
        })
        .and_then(|(stream, _)| {
            // Info hash
            let info_hash = [0; INFO_HASH_SIZE];
            read_exact(stream, info_hash)
        })
        .and_then(|(stream, info_hash)| {
            Ok((stream, InfoHash { hash: info_hash }))
        })
        .map_err(|e| e.into())
        .boxed()
}

/// Read the last half of the peer handshake.
pub fn handshake_read_2_async<R>(stream: R) -> BoxFuture<(R, PeerID), Error>
    where R: AsyncRead + Send + 'static
{
    use tokio_io::io::read_exact;

    // Peer id
    read_exact(stream, [0; PEERID_SIZE])
        .map(|(stream, peer_id)| (stream, PeerID { id: peer_id }))
        .map_err(|e| e.into())
        .boxed()
}

/// Sends one side of the peer handshake.
/// Used both for starting and handling connections.
/// I have no idea whether this flushes the stream.
pub fn handshake_send_async<W>(stream: W, info_hash: InfoHash, peer_id: PeerID) -> BoxFuture<W, Error>
    where W: AsyncWrite + Send + 'static
{
    use tokio_io::io::write_all;

    // Protocol string
    let pstr = HANDSHAKE_PROTOCOL.as_bytes();
    let pstrlen: u8 = pstr.len() as u8;

    write_all(stream, [pstrlen])
        .and_then(move |(stream, _)| write_all(stream, pstr))
        .and_then(|(stream, _)| {
                      // 8 reserved bytes
                      write_all(stream, [0; 8])
                  })
        .and_then(move |(stream, _)| {
                      // Info hash
                      write_all(stream, info_hash.hash)
                  })
        .and_then(move |(stream, _)| {
                      // Peer id
                      write_all(stream, peer_id.id)
                  })
        .map(|(stream, _)| stream)
        .map_err(|e| e.into())
        .boxed()
}

#[cfg(test)]
mod tests {
    use peer_protocol::*;

    // #[test]
    fn test_reader() {
        use util::ReadWire;
        // This test is to understand how Read.take works when you don't use it all.
        let sample = vec![0, 1, 2, 3, 4, 5, 6, 7, 8];
        let mut reader = sample.as_slice();
        let mut reader2 = std::io::Read::take(reader, 1);
        assert_eq!(reader2.read_n(3).unwrap(), vec![0, 1, 2]);
    }
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
        block: Vec<u8>,
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
            }
            &Message::Piece {
                ref piece,
                ref offset,
                ref block,
            } => {
                format!("Piece {{ piece:{} offset:{} block:[len {}] }}",
                        piece,
                        offset,
                        block.len())
            }
            _ => format!("{:?}", self),
        }
    }

    pub fn message_id(&self) -> u8 {
        match *self {
            Message::KeepAlive => 0,
            Message::Choke => 0,
            Message::Unchoke => 1,
            Message::Interested => 2,
            Message::NotInterested => 3,
            Message::Have { piece: _ } => 4,
            Message::Bitfield { bits: _ } => 5,
            Message::Request {
                piece: _,
                offset: _,
                length: _,
            } => 6,
            Message::Piece {
                piece: _,
                offset: _,
                block: _,
            } => 7,
            Message::Cancel {
                piece: _,
                offset: _,
                length: _,
            } => 8,
            Message::Port { port: _ } => 9,
        }
    }
}
