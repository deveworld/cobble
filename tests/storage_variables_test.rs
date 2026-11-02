mod common;
use common::mcsim::Sim;

#[test]
fn integer_variables_are_authoritative_in_storage() {
    let mut sim = Sim::load(
        r#"count = 1
def bump():
    global count
    count += 2
    local = 7
    /data modify storage sim:global vars.count set value 11
    result = count + 1
    /say {result}
"#,
        &[],
    );
    sim.call("bump");
    assert_eq!(sim.log, ["Server: 12"]);
    assert_eq!(sim.storage["vars"]["count"], serde_json::json!(11));
    assert_eq!(sim.storage["locals"]["bump"]["local"], serde_json::json!(7));
    assert_eq!(
        sim.storage["locals"]["bump"]["result"],
        serde_json::json!(12)
    );
}

#[test]
fn raw_score_writes_and_store_callbacks_update_storage() {
    let mut sim = Sim::load(
        r#"count = 1
def main(amount):
    global count
    /execute if score count temp matches 1 run scoreboard players add count temp {amount}
    /execute if score count temp matches 999 run scoreboard players set count temp 100
    store result score count temp:
        /scoreboard players set witness temp 8
    /say {count}
"#,
        &[],
    );
    sim.call_with("main", serde_json::json!({"amount": 3}));
    assert_eq!(sim.storage["vars"]["count"], serde_json::json!(8));
    assert_eq!(sim.log, ["Server: 8"]);
}
