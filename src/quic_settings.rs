use msquic::ServerResumptionLevel;
use msquic::Settings;

pub fn get_quic_settings() -> Settings {
    // Settings::new().set_IdleTimeoutMs(100000)

    Settings::new()
        .set_ServerResumptionLevel(ServerResumptionLevel::ResumeAndZerortt)
        .set_PeerBidiStreamCount(10)
        .set_PeerUnidiStreamCount(1000)
}
