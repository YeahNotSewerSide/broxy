use http::Uri;

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
