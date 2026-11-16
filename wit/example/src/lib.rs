//! `probe`, the example protocol: one beat a cycle, and nothing else. It is what a wasm protocol
//! looks like (wit/protocol.wit), and what src/wasm.rs is tested against.

#[allow(warnings)]
mod bindings;

use std::cell::RefCell;
use std::mem::MaybeUninit;

use bindings::exports::wtf::protocol::api::{
    Card, Field, Found, Guest, Out, Role, StreamDesc, Value,
};

struct Component;

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

/// What `create` made, `feed` carries: the count so far, and how hard a call works.
struct State {
    /// The beats so far.
    n: u64,
    /// Loop turns a feed spends, to test the fuel cap.
    spin: u64,
    /// Bytes held a cycle, to test the memory cap.
    hog: usize,
    /// What `hog` holds: allocated, never read (zeroing it would spend the fuel the cap test needs).
    hold: Vec<Vec<MaybeUninit<u8>>>,
}

impl Default for State {
    fn default() -> Self {
        State {
            n: 0,
            spin: 0,
            hog: 0,
            hold: Vec::new(),
        }
    }
}

/// A field of the stream a beat is a record of.
fn field(name: &str, width: Option<u32>) -> Field {
    Field {
        name: name.to_string(),
        width,
        time: false,
        signed: false,
    }
}

impl Guest for Component {
    fn describe() -> Result<Card, String> {
        Ok(Card {
            about:
                "one beat a cycle, from a component: the example a wasm protocol is written from"
                    .to_string(),
            example: "decode p probe clock=aclk".to_string(),
            clocked: true,
            sides: Vec::new(),
            roles: vec![Role {
                name: "wide".to_string(),
                required: false,
                default: None,
                about: "a one-bit role: probe refuses a wider one".to_string(),
            }],
            params: vec!["spin".to_string(), "hog".to_string()],
        })
    }

    fn create(
        roles: Vec<(String, Option<u32>)>,
        params: Vec<(String, String)>,
    ) -> Result<Vec<StreamDesc>, String> {
        for (name, width) in &roles {
            if let Some(w) = *width {
                if w != 1 {
                    return Err(format!("probe reads one-bit roles; {name} is {w} bits"));
                }
            }
        }
        let mut state = State::default();
        for (name, value) in &params {
            let n: usize = value
                .parse()
                .map_err(|_| format!("{name}: not a number: {value}"))?;
            match name.as_str() {
                "spin" => state.spin = n as u64,
                "hog" => state.hog = n << 20,
                _ => return Err(format!("probe takes no param {name}")),
            }
        }
        STATE.with(|s| *s.borrow_mut() = state);
        // the host reports a trap as rule `host`, so a component that can trap needs a stream that
        // holds violations, with the fields the host fills in by name
        Ok(vec![
            StreamDesc {
                name: "beat".to_string(),
                fields: vec![field("n", Some(64))],
                violations: false,
                about: vec!["one record a cycle, the count so far in n".to_string()],
            },
            StreamDesc {
                name: "errors".to_string(),
                fields: vec![
                    field("rule", None),
                    field("detail", None),
                    field("severity", None),
                ],
                violations: true,
                about: vec!["what the host reports: a trap, or a cap hit".to_string()],
            },
        ])
    }

    fn feed(start: u64, end: u64, _roles: Vec<Option<Value>>) -> Vec<Out> {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            let mut x = 0u64;
            for i in 0..s.spin {
                x = x.wrapping_mul(31).wrapping_add(i);
            }
            std::hint::black_box(x);
            if s.hog > 0 {
                let mut block: Vec<MaybeUninit<u8>> = Vec::with_capacity(s.hog);
                // uninitialised: the memory cap is what this measures, not the cost of zeroing it
                unsafe { block.set_len(s.hog) };
                s.hold.push(block);
            }
            let n = s.n;
            s.n += 1;
            vec![Out::Found(Found {
                stream_name: "beat".to_string(),
                start,
                end,
                fields: vec![Some(Value::Bits(format!("{n:064b}")))],
            })]
        })
    }

    fn finish() -> Vec<Out> {
        Vec::new()
    }
}

bindings::export!(Component with_types_in bindings);
