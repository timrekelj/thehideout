//! Voice calls of [`Client`]: joining and leaving voice channels, own flags and moderation.

use connectrpc::client::HttpClient;

use crate::client::{Client, required};
use crate::error::ApiError;
use crate::proto::thehideout::v1 as pb;
use crate::types::{VoiceJoin, VoiceState};

type Result<T> = std::result::Result<T, ApiError>;

impl Client {
    fn voice(&self) -> Result<pb::VoiceServiceClient<HttpClient>> {
        Ok(pb::VoiceServiceClient::new(self.http.clone(), self.config()?))
    }

    /// Joins a voice channel (needs `CONNECT`) and returns the LiveKit url and token. Joining
    /// another channel leaves the old one; joining the current one reissues a token.
    /// [`ApiError::Precondition`] for a text channel; [`ApiError::Network`] when the server
    /// has no voice configured.
    pub async fn join_voice(&self, channel_id: u64) -> Result<VoiceJoin> {
        let res = self.voice()?.join_voice(pb::JoinVoiceRequest { channel_id, ..Default::default() }).await?.into_owned();
        Ok(VoiceJoin { url: res.url, token: res.token, channel_id: res.channel_id })
    }

    /// Leaves the current voice channel; a no-op when in none.
    pub async fn leave_voice(&self) -> Result<()> {
        self.voice()?.leave_voice(pb::LeaveVoiceRequest::default()).await?;
        Ok(())
    }

    /// Changes your own voice flags; `None` leaves a flag unchanged. [`ApiError::Precondition`]
    /// when not in voice.
    pub async fn update_voice_state(
        &self,
        self_mute: Option<bool>,
        self_deaf: Option<bool>,
        self_video: Option<bool>,
        self_stream: Option<bool>,
    ) -> Result<VoiceState> {
        let res = self
            .voice()?
            .update_voice_state(pb::UpdateVoiceStateRequest { self_mute, self_deaf, self_video, self_stream, ..Default::default() })
            .await?
            .into_owned();
        required(res.state.into_option())
    }

    /// Server-mutes or unmutes a member who is in voice (needs `MUTE_MEMBERS` and a higher top role).
    pub async fn server_mute_member(&self, guild_id: u64, user_id: u64, muted: bool) -> Result<VoiceState> {
        let res = self
            .voice()?
            .server_mute_member(pb::ServerMuteMemberRequest { guild_id, user_id, muted, ..Default::default() })
            .await?
            .into_owned();
        required(res.state.into_option())
    }

    /// Moves a member to another voice channel of the guild (needs `MOVE_MEMBERS` and a higher
    /// top role). The member's client rejoins on [`Event::VoiceMoved`](crate::Event::VoiceMoved).
    pub async fn move_member(&self, guild_id: u64, user_id: u64, channel_id: u64) -> Result<VoiceState> {
        let res = self
            .voice()?
            .move_member(pb::MoveMemberRequest { guild_id, user_id, channel_id, ..Default::default() })
            .await?
            .into_owned();
        required(res.state.into_option())
    }

    /// Removes a member from their voice channel (same rules as [`move_member`](Self::move_member)).
    pub async fn disconnect_member(&self, guild_id: u64, user_id: u64) -> Result<()> {
        self.voice()?.disconnect_member(pb::DisconnectMemberRequest { guild_id, user_id, ..Default::default() }).await?;
        Ok(())
    }
}
