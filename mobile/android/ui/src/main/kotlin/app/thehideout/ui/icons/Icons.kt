package app.thehideout.ui.icons

import androidx.compose.foundation.layout.size
import androidx.compose.material3.Icon
import androidx.compose.material3.LocalContentColor
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.res.painterResource
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.annotation.DrawableRes
import app.thehideout.ui.R

/** Material Symbols (outlined, weight 400), as vector drawables: the native chrome's icon family. */
enum class Sym(@DrawableRes val res: Int) {
    Add(R.drawable.ic_add), AlternateEmail(R.drawable.ic_alternate_email), ArrowBack(R.drawable.ic_arrow_back),
    BrightnessAuto(R.drawable.ic_brightness_auto), Check(R.drawable.ic_check),
    ChevronRight(R.drawable.ic_chevron_right),
    Close(R.drawable.ic_close), ContentCopy(R.drawable.ic_content_copy), Contrast(R.drawable.ic_contrast),
    Code(R.drawable.ic_code), DarkMode(R.drawable.ic_dark_mode), Delete(R.drawable.ic_delete), DensityMedium(R.drawable.ic_density_medium),
    Description(R.drawable.ic_description), Dns(R.drawable.ic_dns), Download(R.drawable.ic_download), Draft(R.drawable.ic_draft),
    Edit(R.drawable.ic_edit), ExpandMore(R.drawable.ic_expand_more), Fingerprint(R.drawable.ic_fingerprint),
    FolderZip(R.drawable.ic_folder_zip), FormatSize(R.drawable.ic_format_size), Forum(R.drawable.ic_forum),
    Fullscreen(R.drawable.ic_fullscreen), Headphones(R.drawable.ic_headphones),
    Home(R.drawable.ic_home), Image(R.drawable.ic_image), LightMode(R.drawable.ic_light_mode), Lock(R.drawable.ic_lock), Menu(R.drawable.ic_menu),
    MicOff(R.drawable.ic_mic_off), Mic(R.drawable.ic_mic), Mood(R.drawable.ic_mood), MoreVert(R.drawable.ic_more_vert),
    MusicNote(R.drawable.ic_music_note),
    Notifications(R.drawable.ic_notifications), OpenInNew(R.drawable.ic_open_in_new), Palette(R.drawable.ic_palette),
    Pause(R.drawable.ic_pause), Person(R.drawable.ic_person), PhotoCamera(R.drawable.ic_photo_camera), PictureAsPdf(R.drawable.ic_picture_as_pdf), PlayArrow(R.drawable.ic_play_arrow),
    Public(R.drawable.ic_public),
    Refresh(R.drawable.ic_refresh),
    Reply(R.drawable.ic_reply), ScreenShare(R.drawable.ic_screen_share), Search(R.drawable.ic_search),
    Send(R.drawable.ic_send), Settings(R.drawable.ic_settings), Share(R.drawable.ic_share), Stack(R.drawable.ic_stack), Tag(R.drawable.ic_tag),
    TextFields(R.drawable.ic_text_fields), Tune(R.drawable.ic_tune), Videocam(R.drawable.ic_videocam),
    Visibility(R.drawable.ic_visibility), VolumeOff(R.drawable.ic_volume_off), VolumeUp(R.drawable.ic_volume_up),
    Warning(R.drawable.ic_warning),
}

/** A [Sym] tinted with the current content colour (or [tint]). Decorative unless [contentDescription] is given. */
@Composable
fun SymIcon(
    sym: Sym,
    contentDescription: String?,
    modifier: Modifier = Modifier,
    size: Dp = 24.dp,
    tint: Color = LocalContentColor.current,
) {
    Icon(
        painter = painterResource(sym.res),
        contentDescription = contentDescription,
        modifier = modifier.size(size),
        tint = tint,
    )
}
