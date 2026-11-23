(function () {
  'use strict';

  // Set this to the web app's URL to turn the "Coming soon" card into an
  // "Open in browser" button. Leave it empty while there is no web app.
  var WEB_APP_URL = '';

  var API = 'https://api.github.com/repos/timrekelj/thehideout/releases/latest';

  function detectOS() {
    var ua = navigator.userAgent || '';
    var p = (navigator.userAgentData && navigator.userAgentData.platform) || navigator.platform || '';
    var s = p + ' ' + ua;
    if (/iPhone|iPad|iPod|Android/i.test(s)) return 'mobile';
    // iPadOS 13+ reports a Macintosh UA; it is the only one with a touch screen.
    if (/Macintosh|MacIntel/i.test(s) && navigator.maxTouchPoints > 1) return 'mobile';
    if (/Mac/i.test(s)) return 'macos';
    if (/Win/i.test(s)) return 'windows';
    if (/Linux|X11|CrOS/i.test(s)) return 'linux';
    return '';
  }

  var os = detectOS();
  var groups = document.querySelectorAll('[data-os-group]');
  Array.prototype.forEach.call(groups, function (g) {
    if (g.getAttribute('data-os-group') === os) {
      g.classList.add('mine');
      var first = g.querySelector('.btn');
      if (first) first.classList.add('primary');
    }
  });

  var slot = document.querySelector('[data-web-slot]');
  if (WEB_APP_URL && slot) {
    var a = document.createElement('a');
    a.className = 'btn primary';
    a.href = WEB_APP_URL;
    a.textContent = 'Open in browser';
    slot.parentNode.replaceChild(a, slot);
    var card = document.getElementById('web-app');
    if (card) {
      card.classList.remove('unavailable');
      card.removeAttribute('aria-disabled');
    }
  }

  if (!window.fetch) return;
  fetch(API, { headers: { Accept: 'application/vnd.github+json' } })
    .then(function (r) {
      if (!r.ok) throw new Error('status ' + r.status);
      return r.json();
    })
    .then(function (release) {
      var assets = (release && release.assets) || [];
      Array.prototype.forEach.call(document.querySelectorAll('[data-asset]'), function (el) {
        var suffix = '-' + el.getAttribute('data-asset');
        for (var i = 0; i < assets.length; i++) {
          var name = String(assets[i].name || '');
          var url = String(assets[i].browser_download_url || '');
          if (name.slice(-suffix.length) === suffix && url.indexOf('https://') === 0) {
            el.href = url;
            break;
          }
        }
      });
    })
    .catch(function () { /* keep the releases-page fallback links */ });
})();
