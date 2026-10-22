use super::*;
use crate::client::endpoint::ClientEndpointId;
use crate::protocol::endpoint::EndpointCursorColor;
use crate::terminal_theme::RgbColor;

const ORANGE: RgbColor = RgbColor {
    r: 0xf5,
    g: 0xa9,
    b: 0x7f,
};
const BLUE: RgbColor = RgbColor {
    r: 0x8a,
    g: 0xad,
    b: 0xf4,
};

fn cursor_color(boot_id: &str, color: Option<RgbColor>) -> EndpointCursorColor {
    EndpointCursorColor {
        boot_id: boot_id.into(),
        color: color.map(Into::into),
    }
}

fn local_state(generation: u64) -> ClientShellState {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_endpoint_snapshot_for_generation(
        &ClientEndpointId::Local,
        generation,
        Box::new(snapshot()),
    );
    state.set_pane_surface(surface());
    state
}

fn host_cursor_color(host: &crate::ghostty::Terminal) -> Option<RgbColor> {
    host.effective_cursor_color()
        .unwrap()
        .map(|color| RgbColor {
            r: color.r,
            g: color.g,
            b: color.b,
        })
}

fn present(
    state: &mut ClientShellState,
    client: &mut crate::client::state::ClientState,
    host: &mut crate::ghostty::Terminal,
) -> Vec<u8> {
    let mut output = Vec::new();
    assert!(client.try_present_frame_to(&mut output, state.compose(100, 30).unwrap()));
    host.write(&output);
    output
}

#[test]
fn reported_cursor_color_reaches_composed_frames() {
    let mut state = local_state(3);
    assert_eq!(state.compose(100, 30).unwrap().cursor_color, None);

    assert!(state.set_endpoint_cursor_color(
        &ClientEndpointId::Local,
        3,
        cursor_color("boot-1", Some(ORANGE)),
    ));
    assert_eq!(state.compose(100, 30).unwrap().cursor_color, Some(ORANGE));
    assert!(
        !state.set_endpoint_cursor_color(
            &ClientEndpointId::Local,
            3,
            cursor_color("boot-1", Some(ORANGE)),
        ),
        "an unchanged color needs no repaint"
    );

    assert!(state.set_endpoint_cursor_color(
        &ClientEndpointId::Local,
        3,
        cursor_color("boot-1", None),
    ));
    assert_eq!(state.compose(100, 30).unwrap().cursor_color, None);
}

#[test]
fn an_open_overlay_owns_the_cursor_color() {
    let mut state = local_state(3);
    state.set_endpoint_cursor_color(
        &ClientEndpointId::Local,
        3,
        cursor_color("boot-1", Some(ORANGE)),
    );
    state.overlay = Some(ClientShellOverlay::Onboarding);
    assert_eq!(state.compose(100, 30).unwrap().cursor_color, None);

    state.overlay = None;
    assert_eq!(state.compose(100, 30).unwrap().cursor_color, Some(ORANGE));
}

#[test]
fn cursor_colors_apply_only_to_the_reporting_connection_and_boot() {
    let mut state = local_state(3);
    assert!(!state.set_endpoint_cursor_color(
        &ClientEndpointId::Local,
        2,
        cursor_color("boot-1", Some(ORANGE)),
    ));
    assert_eq!(state.active_endpoint_cursor_color(), None);
    assert!(!state.set_endpoint_cursor_color(
        &ClientEndpointId::Local,
        3,
        cursor_color("boot-0", Some(ORANGE)),
    ));
    assert_eq!(state.active_endpoint_cursor_color(), None);

    // A replacement connection may report its color before its first snapshot.
    state.set_endpoint_cursor_color(
        &ClientEndpointId::Local,
        4,
        cursor_color("boot-1", Some(ORANGE)),
    );
    assert_eq!(state.active_endpoint_cursor_color(), None);
    state.set_endpoint_snapshot_for_generation(&ClientEndpointId::Local, 4, Box::new(snapshot()));
    assert_eq!(state.active_endpoint_cursor_color(), Some(ORANGE));

    // The replacement for a lost connection only reports a color that differs from the
    // default, so the lost connection's color must not survive into it.
    state.mark_endpoint_disconnected(&ClientEndpointId::Local);
    assert_eq!(state.active_endpoint_cursor_color(), None);
    state.set_endpoint_status(&ClientEndpointId::Local, ClientEndpointStatus::Online);
    state.set_endpoint_snapshot_for_generation(&ClientEndpointId::Local, 5, Box::new(snapshot()));
    assert_eq!(state.active_endpoint_cursor_color(), None);
}

#[test]
fn presented_frames_mirror_the_cursor_color_on_the_host() {
    let mut state = local_state(3);
    let mut client = crate::client::state::ClientState::test_new();
    let mut host = crate::ghostty::Terminal::new(100, 30, 0).unwrap();

    present(&mut state, &mut client, &mut host);
    assert_eq!(host_cursor_color(&host), None);

    state.set_endpoint_cursor_color(
        &ClientEndpointId::Local,
        3,
        cursor_color("boot-1", Some(ORANGE)),
    );
    present(&mut state, &mut client, &mut host);
    assert_eq!(host_cursor_color(&host), Some(ORANGE));
    let unchanged = present(&mut state, &mut client, &mut host);
    assert!(
        !unchanged.windows(4).any(|window| window == b"\x1b]12"),
        "an unchanged color is not resent"
    );

    state.set_endpoint_cursor_color(
        &ClientEndpointId::Local,
        3,
        cursor_color("boot-1", Some(BLUE)),
    );
    present(&mut state, &mut client, &mut host);
    assert_eq!(host_cursor_color(&host), Some(BLUE));

    state.set_endpoint_cursor_color(&ClientEndpointId::Local, 3, cursor_color("boot-1", None));
    present(&mut state, &mut client, &mut host);
    assert_eq!(host_cursor_color(&host), None);
}

#[test]
fn a_drawn_host_cursor_keeps_the_default_color() {
    let mut state = local_state(3);
    state.set_endpoint_cursor_color(
        &ClientEndpointId::Local,
        3,
        cursor_color("boot-1", Some(ORANGE)),
    );
    let mut client = crate::client::state::ClientState::test_new();
    let mut host = crate::ghostty::Terminal::new(100, 30, 0).unwrap();

    client.draw_host_cursor = true;
    present(&mut state, &mut client, &mut host);
    assert_eq!(host_cursor_color(&host), None);

    client.draw_host_cursor = false;
    present(&mut state, &mut client, &mut host);
    assert_eq!(host_cursor_color(&host), Some(ORANGE));
}
