// Copyright 2018-2024 the Deno authors. All rights reserved. MIT license.

use monch::*;

pub fn parse_json(
  input: &str,
) -> Result<serde_json::Map<String, serde_json::Value>, ParseErrorFailureError>
{
  monch::with_failure_handling(|input| {
    let (input, _) = skip_whitespace(input)?;
    let (input, result) = parse_object(input)?;
    let (input, _) = skip_whitespace(input)?;
    Ok((input, result))
  })(input)
}

fn parse_value(input: &str) -> ParseResult<serde_json::Value> {
  match input.chars().next() {
    Some('{') => map(parse_object, serde_json::Value::Object)(input),
    Some('[') => map(parse_array, serde_json::Value::Array)(input),
    Some('"') => map(parse_string, serde_json::Value::String)(input),
    Some(_) => monch::ParseError::fail(input, "unexpected token"),
    None => monch::ParseError::fail(input, "unexpected end of file"),
  }
}

fn parse_object(
  input: &str,
) -> ParseResult<serde_json::Map<String, serde_json::Value>> {
  let (input, _) = ch('{')(input)?;
  let (mut input, _) = skip_whitespace(input)?;
  let mut result = serde_json::Map::new();
  while !input.starts_with('}') || input.is_empty() {
    let (new_input, (key, value)) = parse_object_key(input)?;
    result.insert(key, value);
    let (new_input, _) = skip_whitespace(new_input)?;
    input = new_input;
  }
  let (input, _) = ch('}')(input)?;
  Ok((input, result))
}

fn parse_object_key(input: &str) -> ParseResult<(String, serde_json::Value)> {
  let (input, key) = parse_string(input)?;
  let (input, _) = skip_whitespace(input)?;
  let (input, _) = ch(':')(input)?;
  let (input, _) = skip_whitespace(input)?;
  let (input, value) = parse_value(input)?;
  let (input, _) = maybe(ch(','))(input)?;
  Ok((input, (key, value)))
}

fn parse_array(input: &str) -> ParseResult<Vec<serde_json::Value>> {
  let (input, _) = ch('[')(input)?;
  let (mut input, _) = skip_whitespace(input)?;
  let mut result = Vec::new();

  while !input.starts_with(']') || input.is_empty() {
    let (new_input, value) = parse_value(input)?;
    result.push(value);
    let (new_input, _) = maybe(ch(','))(new_input)?;
    let (new_input, _) = skip_whitespace(new_input)?;
    input = new_input;
  }

  let (input, _) = ch(']')(input)?;
  Ok((input, result))
}

fn parse_string(input: &str) -> ParseResult<String> {
  let (input, _) = ch('"')(input)?;
  let (input, s) = take_while(|c| c != '"')(input)?;
  let (input, _) = ch('"')(input)?;
  Ok((input, s.to_string()))
}
