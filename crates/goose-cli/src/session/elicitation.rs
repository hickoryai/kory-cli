use console::style;
use rmcp::model::ElicitationAction;
use serde_json::Value;
use std::collections::HashMap;
use std::io::{self, BufRead, IsTerminal, Write};
use tokio_util::sync::CancellationToken;

#[cfg(all(test, unix))]
#[path = "../../tests/support/elicitation_signals.rs"]
mod signal_tests;

pub struct ElicitationInput {
    pub action: ElicitationAction,
    pub user_data: HashMap<String, Value>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum SelectChoice {
    Value(String),
    Skip,
}

struct SingleSelect<'a> {
    field_name: &'a str,
    description: Option<&'a str>,
    options: Vec<(SelectChoice, String)>,
    initial_value: Option<SelectChoice>,
}

pub fn collect_elicitation_input(
    message: &str,
    schema: &Value,
    cancel_token: &CancellationToken,
) -> io::Result<ElicitationInput> {
    if cancel_token.is_cancelled() {
        return Ok(cancelled_input());
    }
    // Piped stdin may already contain the next --text conversation turn.
    // CLI elicitation has no separate answer channel for extension forms,
    // so leave queued input to the session and require a terminal here.
    if !io::stdin().is_terminal() {
        return Err(io::Error::new(
            io::ErrorKind::NotConnected,
            "elicitation requires an interactive terminal",
        ));
    }
    let input = collect_elicitation_input_inner(message, schema, cancel_token)?;
    if cancel_token.is_cancelled() {
        return Ok(cancelled_input());
    }
    Ok(input)
}

fn cancelled_input() -> ElicitationInput {
    ElicitationInput {
        action: ElicitationAction::Cancel,
        user_data: HashMap::new(),
    }
}

fn collect_elicitation_input_inner(
    message: &str,
    schema: &Value,
    cancel_token: &CancellationToken,
) -> io::Result<ElicitationInput> {
    if !message.is_empty() {
        println!("\n{}", style(message).cyan());
    }

    let properties = schema.get("properties").and_then(|p| p.as_object());

    if io::stdin().is_terminal() && io::stderr().is_terminal() {
        if let Some(select) = single_select(schema) {
            return prompt_single_select(select);
        }
    }

    let properties = match properties {
        Some(props) if !props.is_empty() => props,
        _ => {
            let prompt = if message.is_empty() {
                "Approve this action?"
            } else {
                "Approve?"
            };
            return match cliclack::confirm(prompt).initial_value(true).interact() {
                Ok(true) => Ok(ElicitationInput {
                    action: ElicitationAction::Accept,
                    user_data: HashMap::new(),
                }),
                Ok(false) => Ok(ElicitationInput {
                    action: ElicitationAction::Decline,
                    user_data: HashMap::new(),
                }),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => Ok(ElicitationInput {
                    action: ElicitationAction::Cancel,
                    user_data: HashMap::new(),
                }),
                Err(e) => Err(e),
            };
        }
    };

    let required: Vec<&str> = schema
        .get("required")
        .and_then(|r| r.as_array())
        .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
        .unwrap_or_default();

    let mut data: HashMap<String, Value> = HashMap::new();
    let interactive = io::stdin().is_terminal() && io::stderr().is_terminal();

    for (name, field_schema) in ordered_fields(properties) {
        if cancel_token.is_cancelled() {
            return Ok(ElicitationInput {
                action: ElicitationAction::Cancel,
                user_data: HashMap::new(),
            });
        }
        let is_required = required.contains(&name.as_str());
        if interactive {
            // A "custom answer" companion (`<field>_custom`, as Claude's AskUserQuestion
            // sends) is offered as the "Other…" choice of the field it belongs to.
            if companion_of(properties, name).is_some() {
                continue;
            }
            let custom = format!("{name}_custom");
            let custom = properties.contains_key(&custom).then_some(custom);
            match prompt_field(
                name,
                field_schema,
                is_required,
                custom.as_deref(),
                &mut data,
            ) {
                Ok(()) => continue,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {
                    return Ok(ElicitationInput {
                        action: ElicitationAction::Cancel,
                        user_data: HashMap::new(),
                    });
                }
                Err(e) => return Err(e),
            }
        }
        let field_type = field_schema
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or("string");
        let description = field_schema.get("description").and_then(|d| d.as_str());
        let default = field_schema.get("default");
        let enum_values = field_schema.get("enum").and_then(|e| e.as_array());

        if field_type == "boolean" {
            let label = match description {
                Some(desc) => format!("{} ({})", name, desc),
                None => name.clone(),
            };
            let default_bool = default.and_then(|v| v.as_bool()).unwrap_or(false);

            match cliclack::confirm(&label)
                .initial_value(default_bool)
                .interact()
            {
                Ok(v) => {
                    data.insert(name.clone(), Value::Bool(v));
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {
                    return Ok(ElicitationInput {
                        action: ElicitationAction::Cancel,
                        user_data: HashMap::new(),
                    });
                }
                Err(e) => return Err(e),
            }
            continue;
        }

        if let Some(options) = enum_values {
            let opts: Vec<&str> = options.iter().filter_map(|v| v.as_str()).collect();
            println!("  {}: {}", style("Options").dim(), opts.join(", "));
        }

        print!("{}", style(name).yellow());
        if let Some(desc) = description {
            print!(" {}", style(format!("({})", desc)).dim());
        }
        if is_required {
            print!("{}", style("*").red());
        }
        if let Some(def) = default {
            print!(" {}", style(format!("[{}]", format_default(def))).dim());
        }
        print!(": ");
        io::stdout().flush()?;

        let input = read_line(cancel_token)?;

        if input.is_none() {
            return Ok(ElicitationInput {
                action: ElicitationAction::Cancel,
                user_data: HashMap::new(),
            });
        }
        let input = input.unwrap();

        let value = if input.is_empty() {
            default.cloned()
        } else {
            Some(parse_value(&input, field_type, enum_values))
        };

        if let Some(v) = value {
            if !v.is_null() {
                data.insert(name.clone(), v);
            }
        }

        if is_required && !data.contains_key(name) {
            println!(
                "{}",
                style(format!("Required field '{}' is missing", name)).red()
            );
            return Ok(ElicitationInput {
                action: ElicitationAction::Decline,
                user_data: HashMap::new(),
            });
        }
    }

    println!();
    Ok(ElicitationInput {
        action: ElicitationAction::Accept,
        user_data: data,
    })
}

/// The form's fields in the order a person reads them: `question_2` before `question_10`.
fn ordered_fields(properties: &serde_json::Map<String, Value>) -> Vec<(&String, &Value)> {
    let mut fields: Vec<_> = properties.iter().collect();
    fields.sort_by_key(|(name, _)| natural_key(name));
    fields
}

fn natural_key(name: &str) -> Vec<(String, u64)> {
    let mut key = Vec::new();
    let mut text = String::new();
    let mut digits = String::new();
    for c in name.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
        } else {
            if !digits.is_empty() {
                key.push((std::mem::take(&mut text), digits.parse().unwrap_or(0)));
                digits.clear();
            }
            text.push(c);
        }
    }
    key.push((text, digits.parse().unwrap_or(0)));
    key
}

/// The choice field a `<field>_custom` free-text field belongs to.
fn companion_of<'a>(
    properties: &'a serde_json::Map<String, Value>,
    name: &str,
) -> Option<&'a Value> {
    let base = name.strip_suffix("_custom")?;
    properties
        .get(base)
        .filter(|field| choices(field).is_some() || array_choices(field).is_some())
}

/// A field's question as a person reads it: its title, else its description, else its name.
fn field_label(name: &str, field: &Value) -> String {
    let title = field.get("title").and_then(Value::as_str);
    let description = field.get("description").and_then(Value::as_str);
    match (title, description) {
        (Some(title), Some(description)) => format!("{title}: {description}"),
        (Some(text), None) | (None, Some(text)) => text.to_string(),
        (None, None) => name.replace('_', " "),
    }
}

/// A single choice's options as (value, label, hint), from a titled `oneOf` or an `enum`.
fn choices(field: &Value) -> Option<Vec<(String, String, String)>> {
    if let Some(one_of) = field.get("oneOf").and_then(Value::as_array) {
        return one_of.iter().map(enum_option).collect();
    }
    let values = field.get("enum")?.as_array()?;
    values
        .iter()
        .map(|value| {
            let value = value.as_str()?;
            Some((value.to_string(), value.to_string(), String::new()))
        })
        .collect()
}

/// A multi-choice (`type: array`) field's options, from `items.anyOf`, `items.oneOf` or
/// `items.enum`.
fn array_choices(field: &Value) -> Option<Vec<(String, String, String)>> {
    if field.get("type").and_then(Value::as_str) != Some("array") {
        return None;
    }
    let items = field.get("items")?;
    match items.get("anyOf").or_else(|| items.get("oneOf")) {
        Some(options) => options.as_array()?.iter().map(enum_option).collect(),
        None => choices(items),
    }
}

fn enum_option(option: &Value) -> Option<(String, String, String)> {
    let value = option.get("const")?.as_str()?;
    let label = option.get("title").and_then(Value::as_str).unwrap_or(value);
    let hint = option
        .get("description")
        .and_then(Value::as_str)
        .unwrap_or_default();
    Some((value.to_string(), label.to_string(), hint.to_string()))
}

#[derive(Clone, PartialEq, Eq)]
enum Pick {
    Value(String),
    Other,
    Skip,
}

/// One field asked with the terminal's own menus and inputs: ↑/↓ and Enter for a choice
/// (with "Other…" for a typed answer and "Skip" when the field is optional), Space to tick
/// a multi-choice, a text box otherwise. Esc or Ctrl-C cancels the whole form.
fn prompt_field(
    name: &str,
    field: &Value,
    required: bool,
    custom: Option<&str>,
    data: &mut HashMap<String, Value>,
) -> io::Result<()> {
    let label = field_label(name, field);
    let field_type = field
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("string");
    if let Some(options) = choices(field) {
        let mut select = cliclack::select(format!("{label} (↑↓ to move, Enter to choose)"));
        for (value, title, hint) in &options {
            select = select.item(Pick::Value(value.clone()), title, hint);
        }
        if custom.is_some() {
            select = select.item(Pick::Other, "Other…", "type your own answer");
        }
        if !required {
            select = select.item(Pick::Skip, "Skip", "");
        }
        if let Some(default) = field.get("default").and_then(Value::as_str) {
            select = select.initial_value(Pick::Value(default.to_string()));
        }
        match select.interact()? {
            Pick::Value(value) => {
                data.insert(name.to_string(), Value::String(value));
            }
            Pick::Other => {
                let answer: String = cliclack::input("Your answer").interact()?;
                if let Some(custom) = custom {
                    data.insert(custom.to_string(), Value::String(answer));
                }
            }
            Pick::Skip => {}
        }
        return Ok(());
    }
    if let Some(options) = array_choices(field) {
        let mut select =
            cliclack::multiselect(format!("{label} (Space to select, Enter to confirm)"))
                .required(required);
        for (value, title, hint) in &options {
            select = select.item(value.clone(), title, hint);
        }
        let picked = select.interact()?;
        data.insert(
            name.to_string(),
            Value::Array(picked.into_iter().map(Value::String).collect()),
        );
        if let Some(custom) = custom {
            let answer: String = cliclack::input("Anything else? (optional)")
                .required(false)
                .interact()?;
            if !answer.trim().is_empty() {
                data.insert(custom.to_string(), Value::String(answer));
            }
        }
        return Ok(());
    }
    if field_type == "boolean" {
        let default = field
            .get("default")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let answer = cliclack::confirm(&label)
            .initial_value(default)
            .interact()?;
        data.insert(name.to_string(), Value::Bool(answer));
        return Ok(());
    }
    let mut input = cliclack::input(&label).required(required);
    let default = field.get("default").map(format_default);
    if let Some(default) = &default {
        input = input.default_input(default);
    }
    if matches!(field_type, "number" | "integer") {
        let integer = field_type == "integer";
        input = input.validate(move |text: &String| {
            let ok = text.is_empty()
                || if integer {
                    text.trim().parse::<i64>().is_ok()
                } else {
                    text.trim().parse::<f64>().is_ok()
                };
            if ok {
                Ok(())
            } else {
                Err("Enter a number")
            }
        });
    }
    let answer: String = input.interact()?;
    if !answer.is_empty() {
        data.insert(name.to_string(), parse_value(&answer, field_type, None));
    }
    Ok(())
}

fn single_select(schema: &Value) -> Option<SingleSelect<'_>> {
    let properties = schema.get("properties")?.as_object()?;
    if properties.len() != 1 {
        return None;
    }

    let (field_name, field_schema) = properties.iter().next()?;
    let description = field_schema.get("description").and_then(Value::as_str);
    let mut options: Vec<(SelectChoice, String)> =
        if let Some(one_of) = field_schema.get("oneOf").and_then(Value::as_array) {
            one_of
                .iter()
                .map(|option| {
                    let value = option.get("const")?.as_str()?;
                    let label = option.get("title").and_then(Value::as_str).unwrap_or(value);
                    Some((SelectChoice::Value(value.to_string()), label.to_string()))
                })
                .collect::<Option<_>>()?
        } else {
            field_schema
                .get("enum")?
                .as_array()?
                .iter()
                .map(|value| {
                    let value = value.as_str()?;
                    Some((SelectChoice::Value(value.to_string()), value.to_string()))
                })
                .collect::<Option<_>>()?
        };

    if options.is_empty() {
        return None;
    }

    let is_required = schema
        .get("required")
        .and_then(Value::as_array)
        .is_some_and(|required| {
            required
                .iter()
                .any(|value| value.as_str() == Some(field_name))
        });
    let default_value = field_schema
        .get("default")
        .and_then(Value::as_str)
        .map(|value| SelectChoice::Value(value.to_string()))
        .filter(|value| options.iter().any(|(option, _)| option == value));

    let initial_value = if !is_required && default_value.is_none() {
        options.push((SelectChoice::Skip, "Skip".to_string()));
        Some(SelectChoice::Skip)
    } else {
        default_value
    };

    Some(SingleSelect {
        field_name,
        description,
        options,
        initial_value,
    })
}

fn prompt_single_select(select: SingleSelect<'_>) -> io::Result<ElicitationInput> {
    let items: Vec<_> = select
        .options
        .iter()
        .map(|(value, label)| (value.clone(), label, ""))
        .collect();
    // The message above already asks the question; a field id like `choice` says nothing.
    let label = match select.description {
        Some(desc) => format!("{desc} (↑↓ to move, Enter to choose)"),
        None => "Choose one (↑↓ to move, Enter to choose)".to_string(),
    };
    let mut prompt = cliclack::select(label).items(&items);
    if let Some(initial_value) = select.initial_value {
        prompt = prompt.initial_value(initial_value);
    }

    match prompt.interact() {
        Ok(SelectChoice::Value(value)) => Ok(ElicitationInput {
            action: ElicitationAction::Accept,
            user_data: HashMap::from([(select.field_name.to_string(), Value::String(value))]),
        }),
        Ok(SelectChoice::Skip) => Ok(ElicitationInput {
            action: ElicitationAction::Accept,
            user_data: HashMap::new(),
        }),
        Err(error) if error.kind() == io::ErrorKind::Interrupted => Ok(ElicitationInput {
            action: ElicitationAction::Cancel,
            user_data: HashMap::new(),
        }),
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
fn read_line(cancel_token: &CancellationToken) -> io::Result<Option<String>> {
    let mut reader = cancellable_stdin::Input::new(io::stdin().lock(), cancel_token)?;
    let result = read_line_from(&mut reader);
    if cancel_token.is_cancelled() || matches!(result, Ok(None)) {
        if matches!(result, Ok(None)) {
            reader.discard_terminal_input()?;
        }
        return Ok(None);
    }
    result
}

#[cfg(not(unix))]
fn read_line(cancel_token: &CancellationToken) -> io::Result<Option<String>> {
    if cancel_token.is_cancelled() {
        return Ok(None);
    }
    let result = read_line_from(&mut io::stdin().lock());
    if cancel_token.is_cancelled() {
        return Ok(None);
    }
    result
}

#[cfg(unix)]
mod cancellable_stdin {
    use super::*;
    use std::io::Read;
    use std::os::fd::AsRawFd;

    pub(super) struct Input<'a> {
        stdin: io::StdinLock<'a>,
        original_flags: libc::c_int,
        cancel_token: &'a CancellationToken,
    }

    impl<'a> Input<'a> {
        pub(super) fn new(
            stdin: io::StdinLock<'a>,
            cancel_token: &'a CancellationToken,
        ) -> io::Result<Self> {
            // Nonblocking reads close the race between readiness and a signal flushing stdin.
            let original_flags = unsafe { libc::fcntl(stdin.as_raw_fd(), libc::F_GETFL) };
            if original_flags < 0
                || unsafe {
                    libc::fcntl(
                        stdin.as_raw_fd(),
                        libc::F_SETFL,
                        original_flags | libc::O_NONBLOCK,
                    )
                } < 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(Self {
                stdin,
                original_flags,
                cancel_token,
            })
        }

        pub(super) fn discard_terminal_input(&self) -> io::Result<()> {
            // A programmatic SIGINT need not flush the terminal's unfinished line.
            if self.stdin.is_terminal()
                && unsafe { libc::tcflush(self.stdin.as_raw_fd(), libc::TCIFLUSH) } < 0
            {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
    }

    impl Read for Input<'_> {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            let available = self.fill_buf()?;
            let count = buffer.len().min(available.len());
            buffer[..count].copy_from_slice(&available[..count]);
            self.consume(count);
            Ok(count)
        }
    }

    impl BufRead for Input<'_> {
        fn fill_buf(&mut self) -> io::Result<&[u8]> {
            loop {
                if self.cancel_token.is_cancelled() {
                    return Err(io::ErrorKind::Interrupted.into());
                }
                // Rustyline's nonterminal input may already have buffered the next answer.
                match self.stdin.fill_buf() {
                    Ok(_) => return self.stdin.fill_buf(),
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
                    Err(error) => return Err(error),
                }
                let mut descriptor = libc::pollfd {
                    fd: self.stdin.as_raw_fd(),
                    events: libc::POLLIN,
                    revents: 0,
                };
                // StdinLock keeps input exclusively owned until the read returns.
                if unsafe { libc::poll(&mut descriptor, 1, 50) } < 0 {
                    return Err(io::Error::last_os_error());
                }
            }
        }

        fn consume(&mut self, amount: usize) {
            self.stdin.consume(amount);
        }
    }

    impl Drop for Input<'_> {
        fn drop(&mut self) {
            // Restore the shared stdin description before handing input back to the CLI.
            unsafe { libc::fcntl(self.stdin.as_raw_fd(), libc::F_SETFL, self.original_flags) };
        }
    }
}

fn read_line_from(reader: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut line = Vec::new();
    loop {
        // BufRead::read_line retries Interrupted instead of allowing cancellation.
        let available = match reader.fill_buf() {
            Ok(available) => available,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => return Ok(None),
            Err(error) => return Err(error),
        };
        if available.is_empty() {
            break;
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let consumed = newline.map_or(available.len(), |index| index + 1);
        line.extend_from_slice(&available[..consumed]);
        reader.consume(consumed);
        if newline.is_some() {
            break;
        }
    }

    let line = String::from_utf8(line).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "stream did not contain valid UTF-8",
        )
    })?;
    Ok(line.ends_with('\n').then(|| line.trim().to_string()))
}

fn format_default(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        _ => value.to_string(),
    }
}

fn parse_value(input: &str, field_type: &str, enum_values: Option<&Vec<Value>>) -> Value {
    if let Some(options) = enum_values {
        let valid: Vec<&str> = options.iter().filter_map(|v| v.as_str()).collect();
        if valid.contains(&input) {
            return Value::String(input.to_string());
        }
        if let Ok(idx) = input.parse::<usize>() {
            if idx > 0 && idx <= valid.len() {
                return Value::String(valid[idx - 1].to_string());
            }
        }
    }

    match field_type {
        "boolean" => {
            let lower = input.to_lowercase();
            Value::Bool(matches!(lower.as_str(), "true" | "yes" | "y" | "1"))
        }
        "integer" => input
            .parse::<i64>()
            .map(|n| Value::Number(n.into()))
            .unwrap_or(Value::Null),
        "number" => input
            .parse::<f64>()
            .ok()
            .and_then(serde_json::Number::from_f64)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        _ => Value::String(input.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::io::{BufReader, Cursor, Read};
    use test_case::test_case;

    #[test_case(json!({}); "confirmation")]
    #[test_case(json!({"properties": {"answer": {"type": "boolean"}}}); "boolean")]
    #[test_case(json!({"properties": {"answer": {"enum": ["yes", "no"]}}}); "selection")]
    #[test_case(json!({"properties": {"answer": {"type": "string"}}}); "freeform")]
    fn cancelled_token_never_collects_answers(schema: Value) {
        let token = CancellationToken::new();
        token.cancel();
        let result = collect_elicitation_input("", &schema, &token).unwrap();
        assert_eq!(result.action, ElicitationAction::Cancel);
        assert!(result.user_data.is_empty());
    }

    struct ScriptedReader {
        prefix: Cursor<&'static [u8]>,
        error: Option<io::ErrorKind>,
        suffix: Cursor<&'static [u8]>,
    }

    impl Read for ScriptedReader {
        fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
            let input = self.fill_buf()?;
            let count = input.len().min(output.len());
            output[..count].copy_from_slice(&input[..count]);
            self.consume(count);
            Ok(count)
        }
    }

    impl BufRead for ScriptedReader {
        fn fill_buf(&mut self) -> io::Result<&[u8]> {
            if self.prefix.position() < self.prefix.get_ref().len() as u64 {
                return self.prefix.fill_buf();
            }
            if let Some(error) = self.error.take() {
                return Err(error.into());
            }
            self.suffix.fill_buf()
        }

        fn consume(&mut self, amount: usize) {
            if self.prefix.position() < self.prefix.get_ref().len() as u64 {
                self.prefix.consume(amount);
            } else {
                self.suffix.consume(amount);
            }
        }
    }

    #[test]
    fn nonterminal_eof_cancels_elicitation_input() {
        assert_eq!(read_line_from(&mut Cursor::new([])).unwrap(), None);
    }

    #[test]
    fn blank_line_accepts_the_field_default() {
        assert_eq!(
            read_line_from(&mut Cursor::new(b"\n")).unwrap(),
            Some(String::new())
        );
    }

    #[test]
    fn partial_line_at_eof_cancels_elicitation_input() {
        assert_eq!(read_line_from(&mut Cursor::new(b"partial")).unwrap(), None);
    }

    #[test_case(""; "before any input")]
    #[test_case("partial"; "after partial input")]
    fn interrupted_read_cancels_elicitation_input(prefix: &'static str) {
        let mut reader = ScriptedReader {
            prefix: Cursor::new(prefix.as_bytes()),
            error: Some(io::ErrorKind::Interrupted),
            suffix: Cursor::new(b"later input\n"),
        };

        assert_eq!(read_line_from(&mut reader).unwrap(), None);
        assert_eq!(
            read_line_from(&mut reader).unwrap(),
            Some("later input".to_string())
        );
    }

    #[test_case(1; "split unicode")]
    #[test_case(64; "multiple lines in one buffer")]
    fn buffered_read_preserves_unicode_and_the_next_line(capacity: usize) {
        let mut reader = BufReader::with_capacity(capacity, Cursor::new(" café \r\nnext\n"));

        assert_eq!(read_line_from(&mut reader).unwrap(), Some("café".into()));
        assert_eq!(read_line_from(&mut reader).unwrap(), Some("next".into()));
        assert_eq!(read_line_from(&mut reader).unwrap(), None);
    }

    #[test_case(b"\xff\n"; "complete line")]
    #[test_case(b"\xff"; "partial line at eof")]
    fn invalid_utf8_returns_an_error(input: &[u8]) {
        assert_eq!(
            read_line_from(&mut Cursor::new(input)).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn non_interruption_errors_are_propagated_without_reading_more() {
        let mut reader = ScriptedReader {
            prefix: Cursor::new(b"partial"),
            error: Some(io::ErrorKind::PermissionDenied),
            suffix: Cursor::new(b"later input\n"),
        };

        assert_eq!(
            read_line_from(&mut reader).unwrap_err().kind(),
            io::ErrorKind::PermissionDenied
        );
        assert_eq!(
            read_line_from(&mut reader).unwrap(),
            Some("later input".to_string())
        );
    }

    #[test]
    fn builds_required_enum_select_with_default() {
        let schema = json!({
            "type": "object",
            "properties": {
                "color": {
                    "type": "string",
                    "enum": ["red", "green"],
                    "default": "green"
                }
            },
            "required": ["color"]
        });

        let select = single_select(&schema).unwrap();

        assert_eq!(select.field_name, "color");
        assert_eq!(
            select.options,
            vec![
                (SelectChoice::Value("red".to_string()), "red".to_string()),
                (
                    SelectChoice::Value("green".to_string()),
                    "green".to_string()
                )
            ]
        );
        assert_eq!(
            select.initial_value,
            Some(SelectChoice::Value("green".to_string()))
        );
    }

    #[test]
    fn optional_select_defaults_to_skip() {
        let schema = json!({
            "type": "object",
            "properties": {
                "color": { "type": "string", "enum": ["", "green"] }
            }
        });

        let select = single_select(&schema).unwrap();

        assert_eq!(select.initial_value, Some(SelectChoice::Skip));
        assert_eq!(select.options.last().unwrap().0, SelectChoice::Skip);
        assert_eq!(
            select.options.first().unwrap().0,
            SelectChoice::Value(String::new())
        );
    }

    #[test]
    fn uses_one_of_titles_as_labels() {
        let schema = json!({
            "type": "object",
            "properties": {
                "size": {
                    "oneOf": [
                        { "const": "s", "title": "Small" },
                        { "const": "l", "title": "Large" }
                    ]
                }
            },
            "required": ["size"]
        });

        let select = single_select(&schema).unwrap();

        assert_eq!(select.options[0].0, SelectChoice::Value("s".to_string()));
        assert_eq!(select.options[0].1, "Small");
        assert_eq!(select.options[1].0, SelectChoice::Value("l".to_string()));
        assert_eq!(select.options[1].1, "Large");
    }

    /// The operator's report: Claude's AskUserQuestion arrived as a bare `question_0:` line
    /// with no options, because the form has a companion "Other" field and only one-field
    /// forms got a menu. Each choice field now gets its options, the companion becomes its
    /// "Other…" choice, and the fields come in reading order.
    #[test]
    fn ask_user_question_forms_read_as_menus() {
        let schema = json!({
            "type": "object",
            "properties": {
                "question_10": {"type": "string", "oneOf": [{"const": "x", "title": "X"}]},
                "question_10_custom": {"type": "string", "title": "Other"},
                "question_2": {
                    "type": "string",
                    "title": "Router fix",
                    "description": "Apply the patches?",
                    "oneOf": [
                        {"const": "Apply", "title": "Apply and install", "description": "Recommended"},
                        {"const": "Decline", "title": "Decline"}
                    ]
                },
                "question_2_custom": {"type": "string", "title": "Other"},
                "question_3": {
                    "type": "array",
                    "items": {"anyOf": [{"const": "a", "title": "A"}, {"const": "b", "title": "B"}]}
                }
            }
        });
        let properties = schema["properties"].as_object().unwrap();
        let order: Vec<&str> = ordered_fields(properties)
            .into_iter()
            .map(|(name, _)| name.as_str())
            .collect();
        assert_eq!(
            order,
            [
                "question_2",
                "question_2_custom",
                "question_3",
                "question_10",
                "question_10_custom"
            ]
        );
        assert!(companion_of(properties, "question_2_custom").is_some());
        assert!(companion_of(properties, "question_2").is_none());
        let question = &properties["question_2"];
        assert_eq!(
            field_label("question_2", question),
            "Router fix: Apply the patches?"
        );
        assert_eq!(field_label("question_9", &json!({})), "question 9");
        assert_eq!(
            choices(question).unwrap(),
            [
                (
                    "Apply".into(),
                    "Apply and install".into(),
                    "Recommended".into()
                ),
                ("Decline".into(), "Decline".into(), String::new())
            ]
        );
        assert_eq!(array_choices(&properties["question_3"]).unwrap().len(), 2);
        assert!(choices(&properties["question_3"]).is_none());
    }

    #[test_case(json!({}); "missing properties")]
    #[test_case(json!({ "properties": {} }); "empty properties")]
    #[test_case(json!({
        "properties": {
            "first": { "enum": ["a"] },
            "second": { "enum": ["b"] }
        }
    }); "multiple properties")]
    #[test_case(json!({
        "properties": { "choice": { "enum": ["a", 2] } }
    }); "non-string enum value")]
    #[test_case(json!({
        "properties": {
            "choice": { "oneOf": [{ "const": "a" }, { "type": "string" }] }
        }
    }); "oneOf branch without const")]
    fn unsupported_schema_does_not_build_select(schema: Value) {
        assert!(single_select(&schema).is_none());
    }
}
