use byteorder::{ByteOrder,BigEndian};
use hyper::Url;
use std::fs::File;
use std::fs;
use std::io;
use std::marker::Send;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use std;
use tokio_core::net;
use tokio_core::net::{TcpStream};
use url::form_urlencoded;
use tokio_core::reactor;
use futures::future::Future;
use futures::future;

use error::{Result};

fn encode(x: &[u8]) -> String {
    // percent_encode(x, QUERY_ENCODE_SET).collect::<String>()
    form_urlencoded::byte_serialize(&x).collect::<String>()
}

// Easier to use version of replace_query_params.
pub struct QueryParameters {
    qps: Vec<(String, String)>,
}

impl QueryParameters {
    pub fn new() -> QueryParameters {
        QueryParameters{
            qps: Vec::new(),
        }
    }

    pub fn push<K, V>(&mut self, k: K, v: V)
        where K: AsRef<[u8]>, V: AsRef<[u8]> {
        let k_enc = encode(k.as_ref());
        let v_enc = encode(v.as_ref());
        self.qps.push((k_enc, v_enc));
    }

    pub fn push_num<K>(&mut self, k: K, v: i64)
        where K: AsRef<[u8]> {
        let k_enc = encode(k.as_ref());
        let v_str = format!("{}", v);
        let v_enc = encode(v_str.as_bytes());
        self.qps.push((k_enc, v_enc));
    }

    // Replace the query parameters of `url`.
    pub fn apply(self, url: &mut Url) {
        let mut s = String::new();
        let mut first = true;
        for (k_enc, v_enc) in self.qps {
            if !first {
                s.push('&')
            }
            first = false;

            s.push_str(&k_enc);
            s.push('=');
            s.push_str(&v_enc);
        }
        url.set_query(Some(&s));
    }
}

// Replace the query parameters of a url.
// This method exists because url's built-in serializer wants to deal in &str's.
// But bittorrent requires the url-encoding of non-utf8 binary data.
// So here we are, with a query parameter method that takes &[u8].
#[allow(dead_code)]
pub fn replace_query_parameters<K, V>(url: &mut Url, query_parameters: &[(K, V)])
    where K: AsRef<[u8]>, V: AsRef<[u8]>
{
    let mut s = String::new();
    let mut first = true;
    for &(ref k, ref v) in query_parameters {
        if !first {
            s.push('&')
        }
        first = false;

        let k_enc = encode(k.as_ref());
        s.push_str(&k_enc);
        s.push('=');
        let v_enc = encode(v.as_ref());
        s.push_str(&v_enc);
    }
    url.set_query(Some(&s));
}

pub trait ReadWire: io::Read {
    fn read_u32(&mut self) -> io::Result<u32>;
    fn read_u8(&mut self) -> io::Result<u8>;
    fn read_n(&mut self, n: u64) -> io::Result<Vec<u8>>;
}

impl<R> ReadWire for R
    where R: io::Read
{
    fn read_u32(&mut self) -> io::Result<u32> {
        let mut buf = [0; 4];
        self.read_exact(&mut buf)?;
        Ok(BigEndian::read_u32(&buf))
    }

    fn read_u8(&mut self) -> io::Result<u8> {
        let mut buf = [0; 1];
        self.read_exact(&mut buf)?;
        Ok(buf[0])
    }

    /// Read exactly n bytes into the returned vec
    fn read_n(&mut self, n: u64) -> io::Result<Vec<u8>> {
        use std::io::Read;
        if n == 0 {
            return Ok(Vec::new());
        }
        let mut buf = Vec::new();
        let mut sub = self.take(n);
        sub.read_to_end(&mut buf)?;
        Ok(buf)
    }
}

#[cfg(test)]
mod tests {
    use util::*;

    #[test]
    fn test_read_extensions() {
        let sample = vec![0, 1, 0, 1,
                          9, 9,
                          12];
        let mut reader = sample.as_slice();
        assert_eq!(reader.read_u32().unwrap(), 65537);
        assert_eq!(reader.read_n(2).unwrap(), vec![9, 9]);
        assert_eq!(reader.read_u8().unwrap(), 12);
    }
}

pub fn byte_to_bits(b: u8) -> [bool; 8] {
    let mut b = b;
    let mut z = [false; 8];
    z[7] = b % 2 == 1;
    b >>= 1;
    z[6] = b % 2 == 1;
    b >>= 1;
    z[5] = b % 2 == 1;
    b >>= 1;
    z[4] = b % 2 == 1;
    b >>= 1;
    z[3] = b % 2 == 1;
    b >>= 1;
    z[2] = b % 2 == 1;
    b >>= 1;
    z[1] = b % 2 == 1;
    b >>= 1;
    z[0] = b % 2 == 1;
    z
}

pub fn bits_to_byte(b: [bool; 8]) -> u8 {
    let mut z = 0;
    for i in 0..8 {
        if b[7-i] { z += (2 as u8).pow(i as u32); }
    }
    z
}

macro_rules! matches(
    ($e:expr, $p:pat) => (
        match $e {
            $p => true,
            _ => false
        }
    )
);

pub fn tcp_connect<T>(addr: T, timeout: Duration) -> io::Result<std::net::TcpStream>
    where T: std::net::ToSocketAddrs + Send + 'static
{
    let (tx1, rx) = mpsc::sync_channel(0);
    let tx2 = tx1.clone();
    thread::spawn(move|| {
        let stream = std::net::TcpStream::connect(addr);
        let _ = tx1.send(stream);
    });
    thread::spawn(move|| {
        thread::sleep(timeout);
        let _ = tx2.send(Err(io::Error::new(io::ErrorKind::TimedOut, "tcp connect timed out")));
    });
    rx.recv().unwrap()
}

pub fn tcp_connect2(addr: &SocketAddr, timeout: Duration, handle: &reactor::Handle)
                    -> Box<Future<Item=Option<TcpStream>, Error=io::Error>>
{
    let timeout = reactor::Timeout::new(timeout, handle).expect("tcp_connect2: could not create timeout");
    let timeout = timeout.map(|_|None::<TcpStream>);
    let stream = TcpStream::connect(addr, handle).map(|x|Some(x));
    timeout.select(stream)
        .map(|(v,_)| v)
        .map_err(|(e,_)| e)
        .boxed()
}

pub fn write_atomic<P1, P2, F>(final_path: P1, temp_path: P2, write: F) -> Result<()>
    where P1: AsRef<Path>,
          P2: AsRef<Path>,
          F: FnOnce(&mut File) -> Result<()>
{
    let temp_path = temp_path.as_ref().to_owned();
    {
        let mut temp_file = File::create(temp_path.clone())?;
        write(&mut temp_file)?;
    }
    fs::rename(temp_path, final_path)?;
    Ok(())
}
