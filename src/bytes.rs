#[allow(unused)]
pub trait Buf {
    fn get_u8(&self, offset: usize) -> u8;
    fn get_u16(&self, offset: usize) -> u16;
    fn get_u32(&self, offset: usize) -> u32;
    fn get_u64(&self, offset: usize) -> u64;
}

macro_rules! buf_get_at {
    ($buf:expr, $offset:expr, $ty:ty) => {{
        let offset = $offset;
        <$ty>::from_be_bytes(
            $buf[offset..offset + core::mem::size_of::<$ty>()]
                .try_into()
                .unwrap(),
        )
    }};
}

impl Buf for [u8] {
    fn get_u8(&self, offset: usize) -> u8 {
        self[offset]
    }

    fn get_u16(&self, offset: usize) -> u16 {
        u16::from_be_bytes([self[offset], self[offset + 1]])
    }

    fn get_u32(&self, offset: usize) -> u32 {
        buf_get_at!(self, offset, u32)
    }

    fn get_u64(&self, offset: usize) -> u64 {
        buf_get_at!(self, offset, u64)
    }
}

#[allow(unused)]
pub trait BufRead {
    fn advance(&mut self, cnt: usize);
    fn read_u8(&mut self) -> u8;
    fn read_u16(&mut self) -> u16;
    fn read_u32(&mut self) -> u32;
    fn read_i32(&mut self) -> i32;
    fn read_u64(&mut self) -> u64;
    fn read_i64(&mut self) -> i64;
    fn read_slice(&mut self, len: usize) -> &[u8];

    fn read_str(&mut self, len: usize) -> &str {
        str::from_utf8(self.read_slice(len)).unwrap()
    }
}

macro_rules! buf_read {
    ($buf: expr, $ty:ty) => {{
        let ret = buf_get_at!($buf, 0, $ty);
        $buf.advance(std::mem::size_of::<$ty>());
        ret
    }};
}

impl BufRead for &[u8] {
    fn advance(&mut self, cnt: usize) {
        *self = &self[cnt..];
    }

    fn read_u8(&mut self) -> u8 {
        let ret = self[0];
        self.advance(1);
        ret
    }

    fn read_u16(&mut self) -> u16 {
        let ret = u16::from_be_bytes([self[0], self[1]]);
        self.advance(2);
        ret
    }

    fn read_u32(&mut self) -> u32 {
        buf_read!(self, u32)
    }

    fn read_i32(&mut self) -> i32 {
        buf_read!(self, i32)
    }

    fn read_u64(&mut self) -> u64 {
        buf_read!(self, u64)
    }

    fn read_i64(&mut self) -> i64 {
        buf_read!(self, i64)
    }

    fn read_slice(&mut self, len: usize) -> &[u8] {
        let ret = &self[..len];
        self.advance(len);
        ret
    }
}

#[allow(unused)]
pub trait BufMut {
    fn put_u8(&mut self, offset: usize, n: u8);
    fn put_u16(&mut self, offset: usize, n: u16);
    fn put_u32(&mut self, offset: usize, n: u32);
    fn put_u64(&mut self, offset: usize, n: u64);
    fn put_slice(&mut self, offset: usize, v: &[u8]);
}

macro_rules! buf_put_at {
    ($buf:expr, $offset:expr, $value:expr, $ty:ty) => {{
        let offset = $offset;
        $buf[offset..offset + core::mem::size_of::<$ty>()]
            .copy_from_slice(&<$ty>::to_be_bytes($value))
    }};
}

impl BufMut for [u8] {
    fn put_u8(&mut self, offset: usize, n: u8) {
        self[offset] = n;
    }

    fn put_u16(&mut self, offset: usize, n: u16) {
        buf_put_at!(self, offset, n, u16);
    }

    fn put_u32(&mut self, offset: usize, n: u32) {
        buf_put_at!(self, offset, n, u32);
    }

    fn put_u64(&mut self, offset: usize, n: u64) {
        buf_put_at!(self, offset, n, u64);
    }

    fn put_slice(&mut self, offset: usize, v: &[u8]) {
        self[offset..offset + v.len()].copy_from_slice(v);
    }
}

#[allow(unused)]
pub trait BufWrite {
    fn advance(&mut self, cnt: usize);
    fn write_u8(&mut self, n: u8);
    fn write_u16(&mut self, n: u16);
    fn write_u32(&mut self, n: u32);
    fn write_i32(&mut self, n: i32);
    fn write_u64(&mut self, n: u64);
    fn write_i64(&mut self, n: i64);
    fn write_slice(&mut self, v: &[u8]);
}

impl BufWrite for Vec<u8> {
    fn advance(&mut self, cnt: usize) {
        self.drain(..cnt);
    }

    fn write_u8(&mut self, n: u8) {
        self.push(n);
    }

    fn write_u16(&mut self, n: u16) {
        self.extend_from_slice(&n.to_be_bytes());
    }

    fn write_u32(&mut self, n: u32) {
        self.extend_from_slice(&n.to_be_bytes());
    }

    fn write_i32(&mut self, n: i32) {
        self.extend_from_slice(&n.to_be_bytes());
    }

    fn write_u64(&mut self, n: u64) {
        self.extend_from_slice(&n.to_be_bytes());
    }

    fn write_i64(&mut self, n: i64) {
        self.extend_from_slice(&n.to_be_bytes());
    }

    fn write_slice(&mut self, v: &[u8]) {
        self.extend_from_slice(v);
    }
}

impl BufWrite for &mut [u8] {
    fn advance(&mut self, cnt: usize) {
        assert!(cnt <= self.len());
        unsafe {
            // SAFETY: We have asserted that cnt <= self.len().
            // The new slice will have a length of self.len() - cnt, which is valid.
            let ptr = self.as_mut_ptr().add(cnt);
            let len = self.len() - cnt;
            *self = std::slice::from_raw_parts_mut(ptr, len);
        }
    }

    fn write_u8(&mut self, n: u8) {
        self[0] = n;
        self.advance(1);
    }

    fn write_u16(&mut self, n: u16) {
        self[..2].copy_from_slice(&n.to_be_bytes());
        self.advance(2);
    }

    fn write_u32(&mut self, n: u32) {
        self[..4].copy_from_slice(&n.to_be_bytes());
        self.advance(4);
    }

    fn write_i32(&mut self, n: i32) {
        self[..4].copy_from_slice(&n.to_be_bytes());
        self.advance(4);
    }

    fn write_u64(&mut self, n: u64) {
        self[..8].copy_from_slice(&n.to_be_bytes());
        self.advance(8);
    }

    fn write_i64(&mut self, n: i64) {
        self[..8].copy_from_slice(&n.to_be_bytes());
        self.advance(8);
    }

    fn write_slice(&mut self, v: &[u8]) {
        let len = v.len();
        self[..len].copy_from_slice(v);
        self.advance(len);
    }
}
