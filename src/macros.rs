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

/// Compare strings case insensitive without allocating
#[macro_export]
macro_rules! match_case_insensitive {
    ($scrutinee:expr,  $($lit:literal => $body:expr),*, _ => $fallback:expr $(,)?) => {{
        match $scrutinee {
            $(s if s.eq_ignore_ascii_case($lit) => $body,)*
            _ => $fallback
        }
    }};
}
