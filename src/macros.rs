#[macro_export]
macro_rules! bail {
    ($err:expr) => {
        return Err(Into::into($err))
    };
}

#[macro_export]
macro_rules! ensure {
    ($cond:expr, $err:expr) => {
        if !$cond {
            return Err(Into::into($err));
        }
    };
}
