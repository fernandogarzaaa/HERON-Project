use super::{fail, Result};
use serde_json::Value;
use std::{collections::HashSet, sync::Arc};
#[derive(Clone)]
pub(super) struct RemoteTool {
    pub definition: Value,
    pub input: Arc<jsonschema::Validator>,
    pub output: Option<Arc<jsonschema::Validator>>,
    pub headers: Vec<(Vec<String>, String)>,
}
fn compile(schema: &Value) -> Result<Arc<jsonschema::Validator>> {
    // Bound expanded schema work and reject recursive references that could
    // validate without consuming instance depth (for example not -> $ref #).
    fn local(
        v: &Value,
        root: &Value,
        depth: usize,
        stack: &mut Vec<String>,
        budget: &mut usize,
    ) -> bool {
        if depth > 24 || *budget == 0 {
            return false;
        }
        *budget -= 1;
        match v {
            Value::Object(o) => {
                if o.contains_key("$dynamicRef") {
                    return false;
                }
                if let Some(reference) = o.get("$ref") {
                    let Some(reference) = reference.as_str() else {
                        return false;
                    };
                    let Some(pointer) = reference.strip_prefix('#') else {
                        return false;
                    };
                    if (!pointer.is_empty() && !pointer.starts_with('/'))
                        || stack.iter().any(|s| s == reference)
                    {
                        return false;
                    }
                    let Some(target) = root.pointer(pointer) else {
                        return false;
                    };
                    stack.push(reference.into());
                    let valid = local(target, root, depth + 1, stack, budget);
                    stack.pop();
                    if !valid {
                        return false;
                    }
                }
                o.iter()
                    .filter(|(k, _)| k.as_str() != "$ref")
                    .all(|(k, v)| {
                        if matches!(k.as_str(), "pattern" | "format")
                            && v.as_str().is_some_and(|s| s.len() > 256)
                        {
                            return false;
                        }
                        local(v, root, depth + 1, stack, budget)
                    })
            }
            Value::Array(a) => a.iter().all(|v| local(v, root, depth + 1, stack, budget)),
            _ => true,
        }
    }
    if !schema.is_object()
        || schema.to_string().len() > 64 * 1024
        || !local(schema, schema, 0, &mut Vec::new(), &mut 2048)
    {
        return Err(fail(
            "schema",
            "Tool schema is oversized, too deep, or contains an external reference.",
        ));
    }
    jsonschema::options()
        .with_draft(jsonschema::Draft::Draft202012)
        .build(schema)
        .map(Arc::new)
        .map_err(|_| fail("schema", "Invalid or unsupported tool schema."))
}
pub(super) fn parse(value: Value) -> Result<RemoteTool> {
    let name = value["name"]
        .as_str()
        .ok_or_else(|| fail("manifest", "Tool name is missing."))?;
    if name.is_empty()
        || name.len() > 128
        || name.chars().any(char::is_control)
        || value["inputSchema"]["type"] != "object"
    {
        return Err(fail("manifest", "Invalid MCP tool definition."));
    }
    let mut headers = Vec::new();
    let mut names = HashSet::new();
    fn walk(
        v: &Value,
        path: Vec<String>,
        reachable: bool,
        headers: &mut Vec<(Vec<String>, String)>,
        names: &mut HashSet<String>,
        depth: usize,
    ) -> Result<()> {
        if depth > 48 {
            return Err(fail("schema", "Tool schema is too deep."));
        }
        if let Some(annotation) = v.get("x-mcp-header") {
            let name = annotation.as_str().unwrap_or("");
            let token = |b: u8| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b);
            if !reachable
                || path.is_empty()
                || name.is_empty()
                || name.len() > 64
                || !name.bytes().all(token)
                || !names.insert(name.to_ascii_lowercase())
                || !matches!(v["type"].as_str(), Some("string" | "integer" | "boolean"))
            {
                return Err(fail(
                    "schema",
                    "Invalid or ambiguous MCP header annotation.",
                ));
            }
            headers.push((path.clone(), name.into()));
        }
        if let Some(object) = v.as_object() {
            for (key, child) in object {
                if key == "properties" {
                    if let Some(properties) = child.as_object() {
                        for (key, property) in properties {
                            let mut next = path.clone();
                            next.push(key.clone());
                            walk(property, next, reachable, headers, names, depth + 1)?;
                        }
                    }
                } else if child.is_object() || child.is_array() {
                    walk(child, path.clone(), false, headers, names, depth + 1)?;
                }
            }
        } else if let Some(array) = v.as_array() {
            for child in array {
                walk(child, path.clone(), false, headers, names, depth + 1)?;
            }
        }
        Ok(())
    }
    walk(
        &value["inputSchema"],
        vec![],
        true,
        &mut headers,
        &mut names,
        0,
    )?;
    let input = compile(&value["inputSchema"])?;
    let output = value.get("outputSchema").map(compile).transpose()?;
    Ok(RemoteTool {
        definition: value,
        input,
        output,
        headers,
    })
}
