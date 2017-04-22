use hyper::Url;
// use url::percent_encoding::{percent_encode, QUERY_ENCODE_SET};
use url::form_urlencoded;

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
