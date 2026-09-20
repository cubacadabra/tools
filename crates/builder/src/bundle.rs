use super::*;

pub(crate) fn sdk_source(module: &str) -> Option<&'static str> {
    SDK_MODULES
        .iter()
        .find(|(name, _, _)| *name == module)
        .map(|(_, _, source)| *source)
}
pub(crate) fn bundle_modules(
    entry: &Path,
    source_root: &Path,
    dependencies: &mut Map<String, Value>,
) -> Result<String> {
    let mut modules = BTreeMap::new();
    visit_local(
        entry,
        source_root,
        &mut modules,
        &mut Vec::new(),
        dependencies,
    )?;
    let entry_id = entry
        .strip_prefix(source_root)
        .map_err(|_| BuildError("entry point must be inside source directory".to_owned()))?
        .to_string_lossy()
        .replace('\\', "/");
    let mut output = vec![
        "local __modules = {}".to_owned(),
        "local __routes = {}".to_owned(),
        "local __cache = {}".to_owned(),
        "local __loading = {}".to_owned(),
        String::new(),
    ];
    for (module_id, module) in &modules {
        output.push(format!("-- begin module: {module_id}"));
        output.push(format!("__routes[{}] = {{", json_string(module_id)));
        for (specifier, dependency) in &module.routes {
            output.push(format!(
                "    [{}] = {},",
                json_string(specifier),
                json_string(dependency)
            ));
        }
        output.push("}".to_owned());
        output.push(format!(
            "__modules[{}] = function(require)",
            json_string(module_id)
        ));
        output.push(module.source.clone());
        output.push("end".to_owned());
        output.push(format!("-- end module: {module_id}"));
        output.push("".to_owned());
    }
    output.extend([
        "local function __require(module_id)".to_owned(),
        "    local cached = __cache[module_id]".to_owned(),
        "    if cached ~= nil then".to_owned(),
        "        return cached".to_owned(),
        "    end".to_owned(),
        "    if __loading[module_id] then".to_owned(),
        "        error(\"cyclic bundled require: \" .. module_id)".to_owned(),
        "    end".to_owned(),
        "    local loader = __modules[module_id]".to_owned(),
        "    if loader == nil then".to_owned(),
        "        error(\"bundled module not found: \" .. module_id)".to_owned(),
        "    end".to_owned(),
        "    local routes = __routes[module_id]".to_owned(),
        "    local function module_require(path)".to_owned(),
        "        local dependency_id = routes[path]".to_owned(),
        "        if dependency_id == nil then".to_owned(),
        "            error(\"undeclared bundled require from \" .. module_id .. \": \" .. tostring(path))".to_owned(),
        "        end".to_owned(),
        "        return __require(dependency_id)".to_owned(),
        "    end".to_owned(),
        "    __loading[module_id] = true".to_owned(),
        "    local result = loader(module_require)".to_owned(),
        "    __loading[module_id] = nil".to_owned(),
        "    if result == nil then".to_owned(),
        "        error(\"bundled module returned nil: \" .. module_id)".to_owned(),
        "    end".to_owned(),
        "    __cache[module_id] = result".to_owned(),
        "    return result".to_owned(),
        "end".to_owned(),
        "".to_owned(),
        format!("return __require({})", json_string(&entry_id)),
    ]);
    Ok(output.join("\n"))
}

pub(crate) fn visit_local(
    path: &Path,
    source_root: &Path,
    modules: &mut BTreeMap<String, Module>,
    stack: &mut Vec<String>,
    dependencies: &mut Map<String, Value>,
) -> Result<String> {
    let module_id = path
        .strip_prefix(source_root)
        .map_err(|_| BuildError("required module must stay inside src/".to_owned()))?
        .to_string_lossy()
        .replace('\\', "/");
    let source = fs::read_to_string(path).map_err(io_error("could not read Luau source"))?;
    visit(
        module_id,
        source,
        Some(path),
        source_root,
        modules,
        stack,
        dependencies,
    )
}

pub(crate) fn visit(
    module_id: String,
    source: String,
    path: Option<&Path>,
    source_root: &Path,
    modules: &mut BTreeMap<String, Module>,
    stack: &mut Vec<String>,
    dependencies: &mut Map<String, Value>,
) -> Result<String> {
    if stack.iter().any(|item| item == &module_id) {
        stack.push(module_id.clone());
        return Err(BuildError(format!(
            "cyclic Luau require: {}",
            stack.join(" -> ")
        )));
    }
    if modules.contains_key(&module_id) {
        return Ok(module_id);
    }
    for (line, value) in source.lines().enumerate() {
        if value.trim_start().starts_with("-- @include") {
            return Err(BuildError(format!(
                "{module_id}:{}: @include is no longer supported; use a Luau require()",
                line + 1
            )));
        }
    }
    stack.push(module_id.clone());
    let mut routes = BTreeMap::new();
    for specifier in require_specifiers(&source, &module_id)? {
        let dependency_id = if specifier.starts_with("@cubacadabra/") {
            let dependency_source = sdk_source(&specifier).ok_or_else(|| {
                BuildError(format!("unknown Cubacadabra SDK module: {specifier}"))
            })?;
            dependencies.insert(specifier.clone(), json!({"source": "cubacadabra-preview-sdk", "sha256": sha256_bytes(dependency_source.as_bytes())}));
            visit(
                specifier.clone(),
                dependency_source.to_owned(),
                None,
                source_root,
                modules,
                stack,
                dependencies,
            )?
        } else {
            let requiring_path = path.ok_or_else(|| {
                BuildError(format!(
                    "{module_id}: SDK modules cannot require game source modules"
                ))
            })?;
            let dependency =
                resolve_local_module(&specifier, requiring_path, source_root, &module_id)?;
            visit_local(&dependency, source_root, modules, stack, dependencies)?
        };
        routes.insert(specifier, dependency_id);
    }
    stack.pop();
    modules.insert(module_id.clone(), Module { source, routes });
    Ok(module_id)
}

pub(crate) fn resolve_local_module(
    specifier: &str,
    requiring_path: &Path,
    source_root: &Path,
    module_id: &str,
) -> Result<PathBuf> {
    if !(specifier.starts_with("./") || specifier.starts_with("../")) || specifier.contains('\\') {
        return Err(BuildError(format!(
            "{module_id}: require path must start with './', '../', or '@cubacadabra/' and use forward slashes"
        )));
    }
    let base = requiring_path.parent().unwrap_or(source_root);
    let mut clean = PathBuf::new();
    for component in Path::new(specifier).components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !clean.pop() {
                    return Err(BuildError(format!(
                        "{module_id}: require must stay inside src/"
                    )));
                }
            }
            std::path::Component::Normal(value) => clean.push(value),
            _ => return Err(BuildError(format!("{module_id}: invalid require path"))),
        }
    }
    let unresolved = base.join(clean);
    let candidates = if unresolved.extension().is_some() {
        vec![unresolved.clone()]
    } else {
        vec![
            unresolved.with_extension("luau"),
            unresolved.with_extension("lua"),
            unresolved.join("init.luau"),
            unresolved.join("init.lua"),
        ]
    };
    let matches: Vec<_> = candidates
        .into_iter()
        .filter(|candidate| candidate.is_file())
        .collect();
    match matches.as_slice() {
        [] => Err(BuildError(format!(
            "{module_id}: required module not found: {specifier}"
        ))),
        [match_path] => Ok(match_path.clone()),
        _ => Err(BuildError(format!(
            "{module_id}: required module is ambiguous: {specifier}"
        ))),
    }
}

pub(crate) fn require_specifiers(source: &str, module_id: &str) -> Result<Vec<String>> {
    let mut result = Vec::new();
    let bytes = source.as_bytes();
    let mut cursor = 0;
    let mut previous_token = String::new();
    while cursor < bytes.len() {
        if source[cursor..].starts_with("--") {
            cursor = skip_comment(source, cursor);
            continue;
        }
        let character = bytes[cursor] as char;
        if matches!(character, '\'' | '"' | '`') {
            cursor = skip_quoted(source, cursor, character);
            previous_token = "string".to_owned();
            continue;
        }
        if character.is_ascii_alphabetic() || character == '_' {
            let start = cursor;
            cursor += 1;
            while cursor < bytes.len()
                && ((bytes[cursor] as char).is_ascii_alphanumeric() || bytes[cursor] as char == '_')
            {
                cursor += 1;
            }
            let identifier = &source[start..cursor];
            if identifier != "require" || previous_token == "." || previous_token == ":" {
                previous_token = identifier.to_owned();
                continue;
            }
            let open = skip_space_comments(source, cursor);
            if open >= bytes.len() || bytes[open] as char != '(' {
                continue;
            }
            let argument = skip_space_comments(source, open + 1);
            if argument >= bytes.len() || !matches!(bytes[argument] as char, '\'' | '"') {
                return Err(BuildError(format!(
                    "{module_id}: require paths must be static quoted strings"
                )));
            }
            let quote = bytes[argument] as char;
            let mut end = argument + 1;
            while end < bytes.len() && bytes[end] as char != quote {
                if bytes[end] == b'\\' {
                    return Err(BuildError(format!(
                        "{module_id}: require paths cannot contain escapes"
                    )));
                }
                end += 1;
            }
            if end >= bytes.len() {
                return Err(BuildError(format!(
                    "{module_id}: unterminated require path"
                )));
            }
            let close = skip_space_comments(source, end + 1);
            if close >= bytes.len() || bytes[close] as char != ')' {
                return Err(BuildError(format!(
                    "{module_id}: require must contain exactly one string path"
                )));
            }
            let specifier = source[argument + 1..end].to_owned();
            if !result.contains(&specifier) {
                result.push(specifier);
            }
            previous_token = ")".to_owned();
            cursor = close + 1;
        } else {
            if !character.is_ascii_whitespace() {
                previous_token = character.to_string();
            }
            cursor += 1;
        }
    }
    Ok(result)
}

pub(crate) fn skip_space_comments(source: &str, mut cursor: usize) -> usize {
    while cursor < source.len() {
        if source.as_bytes()[cursor].is_ascii_whitespace() {
            cursor += 1;
        } else if source[cursor..].starts_with("--") {
            cursor = skip_comment(source, cursor);
        } else {
            break;
        }
    }
    cursor
}
pub(crate) fn skip_quoted(source: &str, mut cursor: usize, quote: char) -> usize {
    cursor += 1;
    while cursor < source.len() {
        if source.as_bytes()[cursor] == b'\\' {
            cursor = (cursor + 2).min(source.len());
        } else if source.as_bytes()[cursor] as char == quote {
            return cursor + 1;
        } else {
            cursor += 1;
        }
    }
    source.len()
}
pub(crate) fn skip_comment(source: &str, mut cursor: usize) -> usize {
    cursor += 2;
    while cursor < source.len() && source.as_bytes()[cursor] != b'\n' {
        cursor += 1;
    }
    cursor
}
pub(crate) fn json_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_owned())
}
pub(crate) fn sha256_bytes(bytes: &[u8]) -> String {
    let mut digest = Sha256::new();
    digest.update(bytes);
    format!("{:x}", digest.finalize())
}
