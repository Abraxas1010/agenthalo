use nucleusdb::container::builder::parse_channel_list;
use nucleusdb::container::launcher::{Channel, MonitorConfig};

#[test]
fn parse_channel_list_valid() {
    let channels = parse_channel_list("chat,payments,tools").expect("parse");
    assert_eq!(
        channels,
        vec![Channel::Chat, Channel::Payments, Channel::Tools]
    );
}

#[test]
fn monitor_config_csv() {
    let cfg = MonitorConfig {
        channels: vec![Channel::Chat, Channel::State],
        agent_id: "agent-a".to_string(),
        max_nesting_depth: 3,
    };
    assert_eq!(cfg.channels_csv(), "chat,state");
}
