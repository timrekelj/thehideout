package app.thehideout.ui.shell

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.HorizontalDivider
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.saveable.Saver
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import app.thehideout.ui.components.GroupLabel
import app.thehideout.ui.components.HideoutCard
import app.thehideout.ui.icons.Sym
import app.thehideout.ui.icons.SymIcon
import app.thehideout.ui.theme.Hideout
import java.io.BufferedReader

private data class LicenceEntry(val name: String, val licence: String, val asset: String)

private const val OFL = "SIL Open Font License 1.1"
private const val APACHE_2 = "Apache License 2.0"
private const val BSD_3 = "BSD 3-Clause License"

private val BUNDLED_FONTS = listOf(
    LicenceEntry("Instrument Sans", OFL, "licenses/instrument_sans_ofl.txt"),
    LicenceEntry("JetBrains Mono", OFL, "licenses/jetbrains_mono_ofl.txt"),
)

/** The project's actual direct dependencies (`mobile/android/gradle/libs.versions.toml`), grouped by licence. */
private val OPEN_SOURCE_LIBRARIES = listOf(
    LicenceEntry("Kotlin & kotlinx.coroutines", APACHE_2, "licenses/apache_2_0.txt"),
    LicenceEntry("Jetpack Compose, Material 3 & AndroidX (core, activity, lifecycle, credentials, browser, exifinterface)", APACHE_2, "licenses/apache_2_0.txt"),
    LicenceEntry("connect-kotlin", APACHE_2, "licenses/apache_2_0.txt"),
    LicenceEntry("OkHttp", APACHE_2, "licenses/apache_2_0.txt"),
    LicenceEntry("Coil", APACHE_2, "licenses/apache_2_0.txt"),
    LicenceEntry("Media3 (ExoPlayer)", APACHE_2, "licenses/apache_2_0.txt"),
    LicenceEntry("ZXing core", APACHE_2, "licenses/apache_2_0.txt"),
    LicenceEntry("Protocol Buffers (protobuf-javalite)", BSD_3, "licenses/bsd_3_clause.txt"),
)

private val ALL_LICENCE_ENTRIES = BUNDLED_FONTS + OPEN_SOURCE_LIBRARIES

private sealed interface AboutRoute {
    data object Licences : AboutRoute
    data class LicenceText(val entry: LicenceEntry) : AboutRoute
}

/** [AboutRoute] is not `Parcelable`; a plain name (looked back up in [ALL_LICENCE_ENTRIES]) round-trips it. */
private val AboutRouteSaver = Saver<AboutRoute?, String>(
    save = { route ->
        when (route) {
            AboutRoute.Licences -> "Licences"
            is AboutRoute.LicenceText -> "LicenceText:${route.entry.name}"
            null -> ""
        }
    },
    restore = { key ->
        when {
            key == "Licences" -> AboutRoute.Licences
            key.startsWith("LicenceText:") -> {
                val name = key.removePrefix("LicenceText:")
                ALL_LICENCE_ENTRIES.firstOrNull { it.name == name }?.let { AboutRoute.LicenceText(it) }
            }
            else -> null
        }
    },
)

/** ACCT-03, MOB-16: version, Terms/Privacy (Custom Tabs) and open-source licences. */
@Composable
fun AboutScreen(versionName: String, serverUrl: String, onOpenUrl: (String) -> Unit, onBack: () -> Unit, modifier: Modifier = Modifier) {
    val c = Hideout.colors
    BackHandler(onBack = onBack)
    var route by rememberSaveable(stateSaver = AboutRouteSaver) { mutableStateOf<AboutRoute?>(null) }
    Box(modifier.fillMaxSize()) {
        Column(Modifier.fillMaxSize().background(c.bg).statusBarsPadding().verticalScroll(rememberScrollState()).padding(bottom = 24.dp)) {
            Row(Modifier.fillMaxWidth().padding(start = 4.dp, end = 20.dp, top = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                IconButton(onClick = onBack) { SymIcon(Sym.ArrowBack, "Back") }
                Text("About", style = MaterialTheme.typography.headlineSmall, color = c.text)
            }
            Column(Modifier.padding(20.dp), verticalArrangement = Arrangement.spacedBy(4.dp)) {
                Text("thehideout", style = MaterialTheme.typography.headlineMedium, color = c.accentText)
                Text("Version $versionName", style = MaterialTheme.typography.bodyMedium, color = c.textMuted)
            }
            GroupLabel("Legal")
            HideoutCard(Modifier.padding(horizontal = 16.dp).fillMaxWidth()) {
                Column {
                    AboutRow(Sym.Description, "Terms of Service") { onOpenUrl("$serverUrl/terms") }
                    HorizontalDivider(color = c.borderSubtle)
                    AboutRow(Sym.Description, "Privacy Policy") { onOpenUrl("$serverUrl/privacy") }
                    HorizontalDivider(color = c.borderSubtle)
                    AboutRow(Sym.Draft, "Open-source licences") { route = AboutRoute.Licences }
                }
            }
        }
        when (val r = route) {
            AboutRoute.Licences -> {
                BackHandler { route = null }
                LicencesScreen(onBack = { route = null }, onOpen = { entry -> route = AboutRoute.LicenceText(entry) }, modifier = Modifier.fillMaxSize())
            }
            is AboutRoute.LicenceText -> {
                BackHandler { route = AboutRoute.Licences }
                LicenceTextScreen(r.entry, onBack = { route = AboutRoute.Licences }, modifier = Modifier.fillMaxSize())
            }
            null -> Unit
        }
    }
}

@Composable
private fun LicencesScreen(onBack: () -> Unit, onOpen: (LicenceEntry) -> Unit, modifier: Modifier = Modifier) {
    val c = Hideout.colors
    Column(modifier.fillMaxSize().background(c.bg).statusBarsPadding().verticalScroll(rememberScrollState())) {
        Row(Modifier.fillMaxWidth().padding(start = 4.dp, end = 20.dp, top = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = onBack) { SymIcon(Sym.ArrowBack, "Back") }
            Text("Open-source licences", style = MaterialTheme.typography.headlineSmall, color = c.text)
        }
        GroupLabel("Bundled fonts")
        HideoutCard(Modifier.padding(horizontal = 16.dp).fillMaxWidth()) {
            Column {
                BUNDLED_FONTS.forEachIndexed { i, e ->
                    if (i > 0) HorizontalDivider(color = c.borderSubtle)
                    AboutRow(Sym.FormatSize, e.name, e.licence) { onOpen(e) }
                }
            }
        }
        GroupLabel("Libraries")
        HideoutCard(Modifier.padding(horizontal = 16.dp).fillMaxWidth()) {
            Column {
                OPEN_SOURCE_LIBRARIES.forEachIndexed { i, e ->
                    if (i > 0) HorizontalDivider(color = c.borderSubtle)
                    AboutRow(Sym.Code, e.name, e.licence) { onOpen(e) }
                }
            }
        }
    }
}

@Composable
private fun LicenceTextScreen(entry: LicenceEntry, onBack: () -> Unit, modifier: Modifier = Modifier) {
    val c = Hideout.colors
    val context = LocalContext.current
    val text by produceState("Loading…", entry) {
        value = runCatching { context.assets.open(entry.asset).bufferedReader().use(BufferedReader::readText) }.getOrDefault("Couldn't load the licence text.")
    }
    Column(modifier.fillMaxSize().background(c.bg).statusBarsPadding().verticalScroll(rememberScrollState())) {
        Row(Modifier.fillMaxWidth().padding(start = 4.dp, end = 20.dp, top = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            IconButton(onClick = onBack) { SymIcon(Sym.ArrowBack, "Back") }
            Text(entry.name, style = MaterialTheme.typography.headlineSmall, color = c.text)
        }
        Text(text, style = MaterialTheme.typography.bodySmall, color = c.textMuted, modifier = Modifier.padding(20.dp))
    }
}

@Composable
private fun AboutRow(sym: Sym, title: String, subtitle: String? = null, onClick: () -> Unit) {
    val c = Hideout.colors
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onClick).heightIn(min = 56.dp).padding(horizontal = 16.dp, vertical = 8.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(16.dp),
    ) {
        SymIcon(sym, null, tint = c.accentText)
        Column(Modifier.weight(1f)) {
            Text(title, style = MaterialTheme.typography.bodyLarge, color = c.text)
            if (subtitle != null) Text(subtitle, style = MaterialTheme.typography.bodySmall, color = c.textMuted)
        }
        SymIcon(Sym.ChevronRight, null, tint = c.textFaint)
    }
}
