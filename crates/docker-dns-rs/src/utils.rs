use color_eyre::eyre;
use tokio::task::JoinHandle;

/// Use this when you have a `JoinHandle<Result<T, E>>`
/// and you want to use it with `tokio::try_join!`
/// when the task completes with an `Result::Err`
/// the `JoinHandle` itself will be `Result::Ok` and thus not
/// trigger the `tokio::try_join!`. This function flattens the 2:
/// `Result::Ok(T)` when both the join-handle AND
/// the result of the inner function are `Result::Ok`, and `Result::Err`
/// when either the join failed, or the inner task failed.
#[expect(unused, reason = "Library Code")]
pub(crate) async fn flatten_handle<T, E>(
    handle: JoinHandle<Result<T, E>>,
) -> Result<T, eyre::Report>
where
    E: 'static + Sync + Send,
    eyre::Report: From<E>,
{
    match handle.await {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(error)) => Err(error.into()),
        Err(error) => Err(error.into()),
    }
}

pub fn pretty_print_iter<T, I>(mut iterable: I) -> String
where
    I: Iterator<Item = T>,
    T: std::fmt::Display,
{
    use std::fmt::Write as _;

    let mut result = String::new();

    if let Some(first) = iterable.next() {
        // write the first element without a leading separator
        write!(&mut result, "{}", first).expect("writing to String cannot fail");

        // for each subsequent element, prepend ", " before writing it
        for item in iterable {
            result.push_str(", ");
            write!(&mut result, "{}", item).expect("writing to String cannot fail");
        }
    }

    result
}
