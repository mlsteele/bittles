use hyper::Url;
use hyper;
use metainfo::*;
use ring::rand::SystemRandom;
use std::error::Error;
use std::io::Read;
use std::time::Duration;
use util::{QueryParameters};

// Client to talk to a tracker
#[derive(Debug)]
pub struct TrackerClient {
    metainfo: MetaInfo,
    peer_id: PeerID,
    url: Url,
    client: hyper::client::Client,
    trackerid: Option<String>,
}

impl TrackerClient {
    pub fn new(metainfo: MetaInfo, peer_id: PeerID) -> Result<TrackerClient,Box<Error>> {
        let url = Url::parse(&metainfo.announce)?;
        let mut client = hyper::client::Client::new();
        client.set_read_timeout(Some(Duration::from_secs(10)));
        client.set_write_timeout(Some(Duration::from_secs(10)));
        Ok(TrackerClient {
            metainfo: metainfo,
            peer_id: peer_id,
            url: url,
            client: client,
            trackerid: None,
        })
    }

    pub fn easy_start(&self) -> Result<TrackerResponse,Box<Error>> {
        let req = TrackerRequest {
            info_hash: self.metainfo.info_hash,
            peer_id: self.peer_id,
            port: 6881, // TODO this is not true
            uploaded: 0,
            downloaded: 0,
            left: 0, // TODO
            compact: true,
            no_peer_id: false,
            event: TrackerEvent::Started,
            ip: None,
            numwant: Some(4),
            key: None,
            trackerid: None,
        };
        self.request(&req)
    }

    fn request(&self, req: &TrackerRequest) -> Result<TrackerResponse,Box<Error>> {
        let http_req = self.build_req(req)?;
        let mut http_res = http_req.send()?;
        let res = self.parse_res(&mut http_res)?;
        // TODO store trackerid if present.
        Ok(res)
    }

    fn build_req(&self, req: &TrackerRequest) -> Result<hyper::client::RequestBuilder,Box<Error>> {
        // TODO set user agent
        let mut qps = QueryParameters::new();
        qps.push("info_hash", req.info_hash);
        qps.push("peer_id", req.peer_id);
        qps.push_num("port", req.port);
        qps.push_num("uploaded", req.uploaded);
        qps.push_num("downloaded", req.downloaded);
        qps.push_num("left", req.left);
        if req.compact {
            qps.push("compact", "1");
        }
        if req.no_peer_id {
            qps.push("no_peer_id", "1");
        }
        if let Some(ev) = req.event.str() {
            qps.push("event", ev);
        }
        if let Some(ref ip) = req.ip {
            qps.push("ip", ip);
        }
        if let Some(numwant) = req.numwant {
            qps.push_num("numwant", numwant);
        }
        if let Some(ref key) = req.key {
            qps.push("key", key);
        }
        if let Some(ref trackerid) = req.trackerid {
            qps.push("trackerid", trackerid);
        }

        let mut url = self.url.clone();
        qps.apply(&mut url);
        println!("url: {}", url);
        // http://requestb.in/12ucjm91?info_hash=4%93%06t%EF;%B91%7F%B5%F2c%CC%A80%F5&%85%23[&peer_id=-BI0001%7F%FB)%B9e%D6%81U0%D3(%F70&port=6881&uploaded=0&downloaded=0&left=0&compact=1&event=started&numwant=4

        let http_req = self.client.get(url);
        Ok(http_req)
    }

    fn parse_res(&self, http_res: &mut hyper::client::response::Response) -> Result<TrackerResponse,Box<Error>> {
        let mut buf = String::new();
        http_res.read_to_string(&mut buf)?;
        println!("http_res: {:?}", http_res);
        Ok(TrackerResponse {
            res: buf,
        })
    }
}

pub const PEERID_SIZE: usize = 20;
pub type PeerID = [u8; PEERID_SIZE];

pub fn new_peer_id(rand: &SystemRandom) -> Result<PeerID,Box<Error>> {
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

#[derive(Clone, Copy)]
#[allow(dead_code)]
enum TrackerEvent {
    Started,
    Stopped,
    Completed,
    Periodical,
}

impl TrackerEvent {
    pub fn str(self) -> Option<String> {
        match self {
            TrackerEvent::Started => Some("started".to_string()),
            TrackerEvent::Stopped => Some("stopped".to_string()),
            TrackerEvent::Completed => Some("completed".to_string()),
            TrackerEvent::Periodical => None,
        }
    }
}

// Parameters used in the client->tracker GET request
pub struct TrackerRequest {
    info_hash: InfoHash, // Hash of the 'info' section of the torrent file
    peer_id: PeerID, // Randomly generated peer id
    port: i64, // Port the client is listening. Typically in [6881-6889]
    uploaded: i64, // Bytes uploaded (since the client sent the 'started' event to the tracker)
    downloaded: i64, // Bytes downloaded (since the client sent the 'started' event to the tracker)
    left: i64, // Bytes left until 100% downloaded
    compact: bool, // Support a compact response
    no_peer_id: bool, // Response can omit peer id field in peers dictionary
    event: TrackerEvent,
    ip: Option<String>, // Outwardly-reachable IP of the client
    numwant: Option<i64>, // Number of peers requested
    key: Option<String>, // Identifier for this client with the tracker
    trackerid: Option<String>, // If a previous announce contained a tracker id, it should be set here
}

// The tracker responds with "text/plain" document consisting of a bencoded dictionary
#[derive(Debug)]
pub struct TrackerResponse {
    res: String,
}
