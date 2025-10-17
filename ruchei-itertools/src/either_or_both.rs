//! <https://docs.rs/itertools/0.14.0/itertools/enum.EitherOrBoth.html>

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
pub enum EitherOrBoth<L, R> {
    Both(L, R),
    Left(L),
    Right(R),
}
