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
pub trait BufMut {
    fn put_u8(&mut self, offset: usize, n: u8);
    fn put_u16(&mut self, offset: usize, n: u16);
    fn put_u32(&mut self, offset: usize, n: u32);
    fn put_u64(&mut self, offset: usize, n: u64);
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
}
