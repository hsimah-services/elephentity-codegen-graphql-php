use super::*;
use std::collections::BTreeSet;
fn claim(names: &mut BTreeSet<String>, name: &str, scope: &str) -> Result<()> {
    let mut chars = name.chars();
    if name.starts_with("__")
        || !chars
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err(format!("Invalid GraphQL name {name:?} in {scope}."));
    }
    if !names.insert(name.into()) {
        return Err(format!("GraphQL name {name:?} is duplicated in {scope}."));
    }
    Ok(())
}
fn enumeration(names: &mut BTreeSet<String>, name: &str, values: &Value) -> Result<()> {
    claim(names, name, "types")?;
    let mut members = BTreeSet::new();
    let values = list(values);
    if values.is_empty() {
        return Err(format!("GraphQL enum {name} has no values."));
    }
    for value in values {
        claim(&mut members, &s(value).to_ascii_uppercase(), name)?;
    }
    Ok(())
}
fn reference(schema: &Value, r: &Value) -> Result<()> {
    if r["primitive"].is_null() && schema["types"][s(&r["declaredType"])].is_null() {
        return Err(format!("Unknown declared type {}.", s(&r["declaredType"])));
    }
    Ok(())
}
pub fn validate(schema: &Value) -> Result<()> {
    let mut types = [
        "Int",
        "Float",
        "String",
        "Boolean",
        "ID",
        "Node",
        "PageInfo",
        "RootQuery",
        "RootMutation",
    ]
    .into_iter()
    .map(str::to_owned)
    .collect::<BTreeSet<_>>();
    let mut roots = BTreeSet::new();
    let mut mutations = BTreeSet::new();
    for t in vals(&schema["types"]) {
        if !t["values"].is_null() {
            enumeration(&mut types, s(&t["name"]), &t["values"])?;
        }
    }
    let entities = vals(&schema["entities"]);
    for e in &entities {
        if !exposure(e) {
            continue;
        }
        let n = name(e);
        claim(&mut types, n, "types")?;
        claim(&mut roots, &low(n), "root fields")?;
        claim(&mut roots, &low(&plural(e)), "root fields")?;
        let mut fields = ["id".to_owned(), "databaseId".to_owned()]
            .into_iter()
            .collect::<BTreeSet<_>>();
        for f in vals(&e["fields"]) {
            claim(&mut fields, s(&f["name"]), n)?;
            reference(schema, &f["type"])?;
            if f["type"]["primitive"] == "enum" {
                if !f["enum"]["inlineValues"].is_null() {
                    enumeration(
                        &mut types,
                        &format!("{}{}", s(&e["name"]), cap(s(&f["name"]))),
                        &f["enum"]["inlineValues"],
                    )?;
                } else if schema["types"][s(&f["enum"]["declaredType"])]["values"].is_null() {
                    return Err(format!(
                        "Enum field {}.{} has no enum definition.",
                        n,
                        s(&f["name"])
                    ));
                }
            }
        }
        for edge in vals(&e["edges"]) {
            if exposure(&schema["entities"][s(&edge["to"])]) {
                claim(&mut fields, s(&edge["name"]), n)?;
            }
        }
        for declaring in &entities {
            if !exposure(declaring) {
                continue;
            }
            for edge in vals(&declaring["edges"]) {
                if edge["to"] == e["name"] && !edge["inverse"].is_null() {
                    let inverse = if b(&edge["inverse"]["derived"]) {
                        low(s(&declaring["name"]))
                    } else {
                        s(&edge["inverse"]["name"]).into()
                    };
                    claim(&mut fields, &inverse, n)?;
                }
            }
        }
        for kind in ["create", "update", "delete"] {
            claim(&mut mutations, &format!("{kind}{n}"), "mutations")?;
        }
        for action in vals(&e["actions"]) {
            claim(
                &mut mutations,
                &format!("{}{n}", s(&action["name"])),
                "mutations",
            )?;
            let mut arguments = ["id".to_owned(), "clientMutationId".to_owned()]
                .into_iter()
                .collect::<BTreeSet<_>>();
            for a in vals(&action["arguments"]) {
                claim(&mut arguments, s(&a["name"]), "action arguments")?;
                reference(schema, &a["type"])?;
            }
        }
        // clientMutationId belongs to the Relay envelope, never an entity input.
        if fields.contains("clientMutationId") {
            return Err(format!(
                "{n}.clientMutationId conflicts with the Relay mutation envelope."
            ));
        }
        for query in vals(&e["queries"]) {
            if !exposure(query) {
                continue;
            }
            let field = query["integrations"]["graphql"]["field"]
                .as_str()
                .unwrap_or(s(&query["name"]));
            claim(&mut roots, field, "root fields")?;
            let mut arguments = BTreeSet::new();
            for a in vals(&query["arguments"]) {
                claim(&mut arguments, s(&a["name"]), "query arguments")?;
                reference(schema, &a["type"])?;
            }
        }
    }
    Ok(())
}
