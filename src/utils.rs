use http::Uri;

/// Combines a base URI with an append URI to create a full URI.
/// 
/// This function takes a base URI and an append URI, then combines them
/// by properly handling path concatenation and query parameters.
/// 
/// # Arguments
/// 
/// * `base` - The base URI that provides the scheme, authority, and base path
/// * `append` - The URI to append, typically containing a path and optional query parameters
/// 
/// # Returns
/// 
/// Returns a `Result<Uri, http::Error>` containing the combined URI on success,
/// or an error if the URI construction fails.
/// 
/// # Examples
/// 
/// ```
/// use http::Uri;
/// use broxy::utils::combine_uris;
/// 
/// let base = "https://example.com/api".parse::<Uri>().unwrap();
/// let append = "/users?page=1".parse::<Uri>().unwrap();
/// let combined = combine_uris(&base, &append).unwrap();
/// assert_eq!(combined.to_string(), "https://example.com/api/users?page=1");
/// ```
pub fn combine_uris(base: &Uri, append: &Uri) -> Result<Uri, http::Error> {
    let base_path = base.path();
    let append_path = append.path();
    let append_query = append.query().unwrap_or("");

    let base_path_trimmed = base_path.trim_end_matches('/');
    let append_path_trimmed = append_path.trim_start_matches('/');

    let mut full_path = format!("{}/{}", base_path_trimmed, append_path_trimmed);

    if !append_query.is_empty() {
        full_path.push('?');
        full_path.push_str(append_query);
    }

    if let Some(scheme) = base.scheme_str() {
        let authority = base.authority().map(|a| a.as_str()).unwrap_or("");

        full_path = format!("{}://{}{}", scheme, authority, full_path);
    }
    Ok(full_path.parse::<Uri>()?)
}
