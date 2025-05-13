use msquic_async::msquic::ServerResumptionLevel;
use msquic_async::msquic::Settings;

pub fn get_quic_settings() -> Settings {
    Settings::new()
        .set_ServerResumptionLevel(ServerResumptionLevel::ResumeAndZerortt)
        .set_PeerBidiStreamCount(0)
        .set_PeerUnidiStreamCount(10)
        .set_InitialRttMs(2)
        //  .set_DatagramReceiveEnabled()
        // .set_StreamMultiReceiveEnabled()
        // .set_StreamRecvWindowDefault(500 * 1024)
        // .set_PacingEnabled()
        // .set_MaxWorkerQueueDelayUs(1000)
        // .set_StreamRecvBufferDefault(330 * 1024)
        // .set_MaxStatelessOperations(1000)
        // .set_InitialWindowPackets(10000)
        // set_StreamRecvWindowUnidiDefault(500 * 1024)
        .set_MaxAckDelayMs(1)
}
