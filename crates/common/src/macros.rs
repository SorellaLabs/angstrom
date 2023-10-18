/// useful for when dealing with [`std::task::Poll`]. This
/// generates the simple return based on a function on a value.
/// This Macro
/// ```ignore
/// return_if!(
///     self
///         .guard_net
///         .poll_next_unpin(cx)
///         .filter_map(|poll| poll)
///         .map(|event| Some(SourceMessages::Swarm(event))) => { is_ready() }
/// );
/// ```
///
/// Gets transformed into this
///
/// ```ignore
/// let res = self
///     .guard_net
///     .poll_next_unpin(cx)
///     .filter_map(|poll| poll)
///     .map(|event| Some(SourceMessages::Swarm(event)));
///
/// if res.is_ready() {
///     return res
/// }
/// ```
#[macro_export]
macro_rules! return_if {
    ($value:expr => {$($value_expr:tt)*} map($map_fn:path)) => {
        let res = $value;
        if res.$($value_expr)* {
            return $map_fn(res)
        }
    };
    ($value:expr => {$($value_expr:tt)*}) => {
        let res = $value;
        if res.$($value_expr)* {
            return res
        }
    };
}
