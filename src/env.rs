use color_eyre::eyre::Context;
use color_eyre::Report;
use url::Url;
fn try_parse_env_variable<T>(env_variable_name: &str) -> Result<Option<T>, color_eyre::Report>
where
    T: std::str::FromStr,
    <T as std::str::FromStr>::Err: std::error::Error,
    <T as std::str::FromStr>::Err: std::marker::Send,
    <T as std::str::FromStr>::Err: std::marker::Sync,
    <T as std::str::FromStr>::Err: 'static,
{
    match std::env::var(env_variable_name).map(|s| str::parse::<T>(&s)) {
        Ok(Ok(ct)) => Ok(Some(ct)),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Ok(Err(err)) => Err(color_eyre::Report::wrap_err(
            err.into(),
            format!(
                "Env variable {:?} could not be cast to requested type",
                env_variable_name
            ),
        )),
        Err(std::env::VarError::NotUnicode(err)) => Err(color_eyre::Report::msg(format!(
            "Env variable {:?} could not be cast to String. Orignal value is {:?}",
            env_variable_name, err
        ))),
    }
}

#[allow(dead_code)]
pub fn try_parse_optional_env_variable<T>(
    env_variable_name: &str,
) -> Result<Option<T>, color_eyre::Report>
where
    T: std::fmt::Debug,
    T: std::str::FromStr,
    <T as std::str::FromStr>::Err: std::error::Error,
    <T as std::str::FromStr>::Err: std::marker::Send,
    <T as std::str::FromStr>::Err: std::marker::Sync,
    <T as std::str::FromStr>::Err: 'static,
{
    match try_parse_env_variable(env_variable_name) {
        Ok(Some(ct)) => {
            tracing::info!("{} set to {:?}", env_variable_name, ct);
            Ok(Some(ct))
        },
        Ok(None) => {
            tracing::info!("{} not set", env_variable_name);
            Ok(None)
        },
        Err(e) => Err(e),
    }
}

pub fn try_parse_env_variable_with_default<T>(
    env_variable_name: &str,
    default: T,
) -> Result<T, color_eyre::Report>
where
    T: std::fmt::Debug,
    T: std::str::FromStr,
    <T as std::str::FromStr>::Err: std::error::Error,
    <T as std::str::FromStr>::Err: std::marker::Send,
    <T as std::str::FromStr>::Err: std::marker::Sync,
    <T as std::str::FromStr>::Err: 'static,
{
    match try_parse_env_variable(env_variable_name) {
        Ok(Some(ct)) => {
            tracing::info!("{} set to {:?}", env_variable_name, ct);
            Ok(ct)
        },

        Ok(None) => {
            tracing::info!("{} not set, defaulting to {:?}", env_variable_name, default);
            Ok(default)
        },
        Err(e) => Err(e),
    }
}

#[allow(dead_code)]
pub fn get_env_as_url(key: &str) -> Result<Url, Report> {
    let value = std::env::var(key)?;

    Url::parse(&value).wrap_err_with(|| format!("Couldn't convert {:?} to URL", value))
}
