/// Simple XML writer for S3 responses.
pub struct XmlWriter {
    buf: String,
}

const S3_XMLNS: &str = "http://s3.amazonaws.com/doc/2006-03-01/";

impl XmlWriter {
    pub fn new() -> Self {
        Self {
            buf: String::with_capacity(4096),
        }
    }

    pub fn declaration(&mut self) -> &mut Self {
        self.buf
            .push_str(r#"<?xml version="1.0" encoding="UTF-8"?>"#);
        self
    }

    pub fn open(&mut self, tag: &str) -> &mut Self {
        self.buf.push('<');
        self.buf.push_str(tag);
        self.buf.push('>');
        self
    }

    pub fn open_s3(&mut self, tag: &str) -> &mut Self {
        self.buf.push('<');
        self.buf.push_str(tag);
        self.buf.push_str(" xmlns=\"");
        self.buf.push_str(S3_XMLNS);
        self.buf.push_str("\">");
        self
    }

    pub fn close(&mut self, tag: &str) -> &mut Self {
        self.buf.push_str("</");
        self.buf.push_str(tag);
        self.buf.push('>');
        self
    }

    pub fn elem(&mut self, tag: &str, value: &str) -> &mut Self {
        self.open(tag);
        xml_escape_into(&mut self.buf, value);
        self.close(tag)
    }

    pub fn elem_opt(&mut self, tag: &str, value: &Option<String>) -> &mut Self {
        if let Some(v) = value {
            self.elem(tag, v);
        }
        self
    }

    pub fn elem_bool(&mut self, tag: &str, value: bool) -> &mut Self {
        self.elem(tag, if value { "true" } else { "false" })
    }

    pub fn elem_u64(&mut self, tag: &str, value: u64) -> &mut Self {
        self.elem(tag, &value.to_string())
    }

    pub fn elem_i32(&mut self, tag: &str, value: i32) -> &mut Self {
        self.elem(tag, &value.to_string())
    }

    pub fn raw(&mut self, s: &str) -> &mut Self {
        self.buf.push_str(s);
        self
    }

    pub fn finish(self) -> String {
        self.buf
    }
}

fn xml_escape_into(buf: &mut String, s: &str) {
    for ch in s.chars() {
        match ch {
            '&' => buf.push_str("&amp;"),
            '<' => buf.push_str("&lt;"),
            '>' => buf.push_str("&gt;"),
            '"' => buf.push_str("&quot;"),
            '\'' => buf.push_str("&apos;"),
            _ => buf.push(ch),
        }
    }
}
