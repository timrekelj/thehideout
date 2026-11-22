package app.thehideout.ui.components

import android.content.Context
import android.net.Uri
import androidx.browser.customtabs.CustomTabsIntent
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.expandVertically
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.shrinkVertically
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.rotate
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import app.thehideout.core.attachment.fitSize
import app.thehideout.core.attachment.mediaUrl
import app.thehideout.core.embed.EMBED_BELOW_THUMB_MAX_H
import app.thehideout.core.embed.EMBED_BELOW_THUMB_MAX_W
import app.thehideout.core.embed.EMBED_DESCRIPTION_LINES
import app.thehideout.core.embed.EMBED_MAX_WIDTH_DP
import app.thehideout.core.embed.EMBED_SIDE_THUMB_DP
import app.thehideout.core.embed.EmbedProvider
import app.thehideout.core.embed.embedThumbnailSide
import app.thehideout.core.model.Attachment
import app.thehideout.core.model.Embed
import app.thehideout.core.model.EmbedKind
import app.thehideout.core.model.EmbedMediaKind
import app.thehideout.design.Radius
import app.thehideout.ui.icons.Sym
import app.thehideout.ui.icons.SymIcon
import app.thehideout.ui.theme.Hideout
import java.time.ZoneId
import java.time.format.DateTimeFormatter
import java.time.format.FormatStyle
import java.util.Locale

/** Opens [url] in a Custom Tab (falls back silently if nothing can handle it). */
fun openInCustomTab(context: Context, url: String) {
    runCatching { CustomTabsIntent.Builder().build().launchUrl(context, Uri.parse(url)) }
}

/**
 * A link embed (ATT-06/07): a left rail in the provider's colour (or [Hideout]'s `borderStrong`
 * for a plain link), a header with a site icon, the site name and a fold chevron, and - folded
 * away by the chevron - either a plain link's title/description/thumbnail, or a rich provider's
 * post (author, text, media, "Open original").
 */
@Composable
fun EmbedCard(
    embed: Embed,
    collapsed: Boolean,
    onToggleCollapsed: () -> Unit,
    modifier: Modifier = Modifier,
    onOpenLightbox: (LightboxRequest) -> Unit = {},
) {
    val c = Hideout.colors
    val context = LocalContext.current
    val mediaBase = LocalMediaBase.current
    val provider = remember(embed.provider) { EmbedProvider.fromKey(embed.provider) }
    val isPost = provider != EmbedProvider.Link || embed.kind == EmbedKind.Rich
    val railColor = provider.paletteKey?.let { c.channel(it)?.ink } ?: c.borderStrong
    val site = if (provider == EmbedProvider.Link) embed.siteName.ifEmpty { "Link" } else provider.label
    val title = if (embed.title.isEmpty() && provider == EmbedProvider.Link) embed.url else embed.title
    val chevronRotation by animateFloatAsState(if (collapsed) 0f else 90f, label = "embed-chevron")

    Row(
        modifier
            .padding(top = 4.dp)
            .widthIn(max = EMBED_MAX_WIDTH_DP.dp)
            .fillMaxWidth(),
    ) {
        Box(Modifier.width(3.dp).padding(vertical = 2.dp)) {
            Box(Modifier.fillMaxHeight().clip(RoundedCornerShape(Radius.xs)).background(railColor))
        }
        Column(Modifier.weight(1f).clip(RoundedCornerShape(Radius.lg)).background(c.hover).padding(start = 7.dp, end = 5.dp, top = 2.dp, bottom = 2.dp)) {
            Row(
                Modifier
                    .fillMaxWidth()
                    .heightIn(min = 28.dp)
                    .clip(RoundedCornerShape(Radius.sm))
                    .clickable(onClickLabel = "$site preview") { onToggleCollapsed() }
                    .semantics { role = Role.Button }
                    .padding(horizontal = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
                horizontalArrangement = Arrangement.spacedBy(6.dp),
            ) {
                ProviderGlyph(provider, railColor)
                Text(site, style = MaterialTheme.typography.labelMedium, color = c.textMuted, maxLines = 1, overflow = TextOverflow.Ellipsis)
                if (collapsed && title.isNotEmpty()) {
                    Text(
                        title,
                        style = MaterialTheme.typography.labelMedium,
                        color = c.textFaint,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f),
                    )
                } else {
                    Box(Modifier.weight(1f))
                }
                SymIcon(
                    Sym.ChevronRight,
                    contentDescription = null,
                    size = 14.dp,
                    tint = c.textFaint,
                    modifier = Modifier.rotate(chevronRotation),
                )
            }
            AnimatedVisibility(
                visible = !collapsed,
                enter = fadeIn() + expandVertically(),
                exit = fadeOut() + shrinkVertically(),
            ) {
                Column(Modifier.fillMaxWidth().padding(horizontal = 4.dp, vertical = 2.dp).padding(bottom = 6.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    if (isPost) {
                        PostBody(embed, title, mediaBase, onOpenLightbox) { openInCustomTab(context, embed.url) }
                    } else {
                        LinkBody(embed, title, mediaBase) { openInCustomTab(context, embed.url) }
                    }
                }
            }
        }
    }
}

@Composable
private fun ProviderGlyph(provider: EmbedProvider, railColor: androidx.compose.ui.graphics.Color, modifier: Modifier = Modifier) {
    if (provider == EmbedProvider.Link) {
        SymIcon(Sym.Public, contentDescription = null, size = 14.dp, tint = Hideout.colors.textMuted, modifier = modifier)
    } else {
        // Desktop shows the provider's brand mark; Phosphor's brand set isn't ported to Android
        // (see mobile/README.md "Icons"), so the rail's own colour stands in for it here.
        Box(modifier.size(14.dp).clip(CircleShape).background(railColor))
    }
}

/** A plain link card: the title (opens the URL), a 3-line description, and a thumbnail beside or below. */
@Composable
private fun LinkBody(embed: Embed, title: String, mediaBase: String, onOpenLink: () -> Unit) {
    val c = Hideout.colors
    val side = embedThumbnailSide(embed.thumbnailWidth, embed.thumbnailHeight)
    val texts = @Composable {
        Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
            if (title.isNotEmpty()) {
                Text(
                    title,
                    style = MaterialTheme.typography.titleSmall.copy(fontWeight = FontWeight(650)),
                    color = c.accentText,
                    modifier = Modifier.clickable(onClickLabel = "Open link", role = Role.Button, onClick = onOpenLink),
                )
            }
            if (embed.description.isNotEmpty()) {
                Text(embed.description, style = MaterialTheme.typography.bodyMedium, color = c.textMuted, maxLines = EMBED_DESCRIPTION_LINES, overflow = TextOverflow.Ellipsis)
            }
        }
    }
    val thumbnail = embed.thumbnail
    if (thumbnail == null) {
        texts()
        return
    }
    val url = mediaUrl(mediaBase, thumbnail)
    if (side) {
        Row(horizontalArrangement = Arrangement.spacedBy(12.dp)) {
            Box(Modifier.weight(1f)) { texts() }
            AttachmentImage(url, contentDescription = null, modifier = Modifier.size(EMBED_SIDE_THUMB_DP.dp).clip(RoundedCornerShape(Radius.md)))
        }
    } else {
        Column(verticalArrangement = Arrangement.spacedBy(8.dp)) {
            texts()
            val (w, h) = fitSize(embed.thumbnailWidth?.let { ww -> embed.thumbnailHeight?.let { hh -> ww to hh } }, EMBED_BELOW_THUMB_MAX_W, EMBED_BELOW_THUMB_MAX_H)
            AttachmentImage(url, contentDescription = null, modifier = Modifier.size(w.dp, h.dp).clip(RoundedCornerShape(Radius.md)))
        }
    }
}

/** A rich post (X/TikTok/Instagram/Reddit): author, title/text, media, published time, "Open original". */
@Composable
private fun PostBody(embed: Embed, title: String, mediaBase: String, onOpenLightbox: (LightboxRequest) -> Unit, onOpenOriginal: () -> Unit) {
    val c = Hideout.colors
    if (embed.authorName.isNotEmpty()) {
        Row(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
            embed.authorIcon?.let { AttachmentImage(mediaUrl(mediaBase, it), null, Modifier.size(20.dp).clip(CircleShape)) }
            Text(embed.authorName, style = MaterialTheme.typography.bodyMedium.copy(fontWeight = FontWeight(650)), color = c.text)
        }
    }
    if (title.isNotEmpty()) Text(title, style = MaterialTheme.typography.bodyMedium.copy(fontWeight = FontWeight(650)), color = c.text)
    if (embed.description.isNotEmpty()) MarkdownText(embed.description)
    val images = embed.media.withIndex().filter { (_, m) -> m.kind == EmbedMediaKind.Image }
    if (images.isNotEmpty()) {
        val attachments = images.map { (i, m) -> Attachment(id = i.toLong(), filename = "picture", contentType = "image/jpeg", size = m.size, file = m.file, width = m.width, height = m.height) }
        AttachmentImageGrid(
            images = attachments,
            mediaBase = mediaBase,
            onOpen = { i -> onOpenLightbox(LightboxRequest(attachments.map { LightboxItem(it.file, it.filename, it.width ?: 0, it.height ?: 0, isVideo = false) }, i)) },
        )
    }
    embed.media.withIndex().filter { (_, m) -> m.kind != EmbedMediaKind.Image }.forEach { (i, m) ->
        val attachment = Attachment(id = i.toLong(), filename = "video", contentType = "video/mp4", size = m.size, file = m.file, width = m.width, height = m.height, thumbnail = m.poster)
        AttachmentVideoCard(
            attachment,
            mediaBase,
            onExpand = { pos -> onOpenLightbox(LightboxRequest(listOf(LightboxItem(m.file, "video", m.width, m.height, isVideo = true, startPositionMs = pos)), 0)) },
        )
    }
    Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(12.dp)) {
        embed.publishedAt?.let { at ->
            Text(
                DateTimeFormatter.ofLocalizedDateTime(FormatStyle.SHORT).withLocale(Locale.getDefault()).withZone(ZoneId.systemDefault()).format(at),
                style = MaterialTheme.typography.labelSmall,
                color = c.textMuted,
            )
        }
        Row(
            Modifier.clickable(onClickLabel = "Open original", onClick = onOpenOriginal),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(4.dp),
        ) {
            SymIcon(Sym.OpenInNew, contentDescription = null, size = 14.dp, tint = c.accentText)
            Text("Open original", style = MaterialTheme.typography.labelMedium.copy(fontWeight = FontWeight(650)), color = c.accentText)
        }
    }
}
