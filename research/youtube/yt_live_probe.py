#!/usr/bin/env python3
"""RESEARCH SPIKE - not part of TwitchMiner.

Probe the public (unauthenticated) YouTube innertube endpoints for one live
video and print only the fields relevant to a future "YouTube drops" module:
live status, heartbeat parameters, which stream manifests are offered, which
playback-tracking URLs the player would ping, and any drops/rewards/account
linking hints found in the `next` (watch page) response.

Deliberately NOT done here: no login, no cookies, no `api/stats/*` pings, no
segment download. The script only reads public data, with a pause between
requests (about 5 to 8 requests per run).

Usage:
    python3 -I yt_live_probe.py <video_id | @handle> [--clients web,mweb,tv,android_vr]
                                [--heartbeat] [--dump DIR]
"""

import argparse
import gzip
import json
import os
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

# Public client identifiers (the volatile part; would live in a manifest).
# Versions observed in October 2026; YouTube accepts slightly older ones.
CLIENTS = {
    "web": {
        "id": 1,
        "context": {"clientName": "WEB", "clientVersion": "2.20261023.01.00"},
        "ua": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
        "(KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36",
    },
    "mweb": {
        "id": 2,
        "context": {"clientName": "MWEB", "clientVersion": "2.20261023.01.00"},
        "ua": "Mozilla/5.0 (iPad; CPU OS 16_7_10 like Mac OS X) AppleWebKit/605.1.15 "
        "(KHTML, like Gecko) Version/16.6 Mobile/15E148 Safari/604.1",
    },
    "tv": {
        "id": 7,
        "context": {"clientName": "TVHTML5", "clientVersion": "7.20260707.07.00"},
        "ua": "Mozilla/5.0 (ChromiumStylePlatform) Cobalt/25.lts.30.1034943-gold "
        "(unlike Gecko), Unknown_TV_Unknown_0/Unknown (Unknown, Unknown)",
    },
    "android_vr": {
        "id": 28,
        "context": {
            "clientName": "ANDROID_VR",
            "clientVersion": "1.65.10",
            "deviceMake": "Oculus",
            "deviceModel": "Quest 3",
            "androidSdkVersion": 32,
            "osName": "Android",
            "osVersion": "12L",
        },
        "ua": "com.google.android.apps.youtube.vr.oculus/1.65.10 "
        "(Linux; U; Android 12L; eureka-user Build/SQ3A.220605.009.A1) gzip",
    },
}

BASE = "https://www.youtube.com"
PAUSE_S = 1.5
# Words that would reveal a drops / rewards / account-linking surface.
# "drop" must be a word or a camelCase part ("dropsEnabled", "showDropsBadge",
# "BADGE_DROPS"); "dropShadow", "DROPDOWN", "allowDropping" are layout noise.
HINTS = re.compile(
    r"(?<![A-Za-z])[dD]rops(?![a-z])|(?<=[a-z])Drops(?![a-z])|DROPS|(?<![A-Za-z])[dD]rop(?![A-Za-z])"
    r"|(?i:reward|account.?link|linked.?account|connected.?app|account_sharing|partner)"
)
# Keys that are noise for the hint search (tracking blobs, urls of thumbnails...).
SKIP_KEYS = {"clickTrackingParams", "trackingParams", "thumbnails", "url", "loggingContext"}


def http(method, url, ua, body=None, headers=None):
    data = json.dumps(body).encode() if body is not None else None
    req = urllib.request.Request(url, data=data, method=method)
    req.add_header("User-Agent", ua)
    req.add_header("Accept-Language", "en-US,en;q=0.9")
    req.add_header("Accept-Encoding", "gzip")
    if data is not None:
        req.add_header("Content-Type", "application/json")
    for k, v in (headers or {}).items():
        req.add_header(k, v)
    try:
        with urllib.request.urlopen(req, timeout=20) as r:
            raw = r.read()
            if r.headers.get("Content-Encoding") == "gzip":
                raw = gzip.decompress(raw)
            return r.status, r.geturl(), raw.decode("utf-8", "replace")
    except urllib.error.HTTPError as e:
        return e.code, url, e.read().decode("utf-8", "replace")


def page_config(target):
    """Fetch the watch (or @handle/live) page once: video id, client version, visitor data."""
    url = f"{BASE}/{target}/live" if target.startswith("@") else f"{BASE}/watch?v={target}"
    status, final, html = http("GET", url, CLIENTS["web"]["ua"])
    cfg = {"status": status, "final_url": final}
    # videoDetails is missing when the page answers with the "confirm you're not a bot"
    # interstitial; currentVideoEndpoint still names the live video.
    m = re.search(r'"videoDetails":\{"videoId":"([\w-]{11})"', html) or re.search(
        r'"currentVideoEndpoint":\{.{0,400}?"url":"/watch\?v=([\w-]{11})', html
    )
    cfg["video_id"] = m.group(1) if m else (None if target.startswith("@") else target)
    for key, name in (("INNERTUBE_CLIENT_VERSION", "client_version"), ("VISITOR_DATA", "visitor_data")):
        m = re.search(rf'"{key}":"([^"]+)"', html)
        cfg[name] = m.group(1) if m else None
    m = re.search(r'"playabilityStatus":\{"status":"(\w+)"(?:,"reason":"([^"]*)")?', html)
    cfg["page_playability"] = (m.group(1), m.group(2)) if m else None
    return cfg


def innertube(endpoint, client_key, body, cfg):
    c = CLIENTS[client_key]
    ctx = dict(c["context"], hl="en", gl="US")
    if client_key in ("web", "mweb") and cfg.get("client_version"):
        ctx["clientVersion"] = cfg["client_version"]
    if cfg.get("visitor_data"):
        ctx["visitorData"] = urllib.parse.unquote(cfg["visitor_data"])
    headers = {
        "X-YouTube-Client-Name": str(c["id"]),
        "X-YouTube-Client-Version": ctx["clientVersion"],
        "Origin": BASE,
    }
    if cfg.get("visitor_data"):
        headers["X-Goog-Visitor-Id"] = urllib.parse.unquote(cfg["visitor_data"])
    payload = dict(body, context={"client": ctx})
    status, _, text = http("POST", f"{BASE}/youtubei/v1/{endpoint}?prettyPrint=false", c["ua"], payload, headers)
    try:
        return status, json.loads(text)
    except json.JSONDecodeError:
        return status, {"_raw": text[:300]}


def get(d, *path, default=None):
    for p in path:
        if isinstance(d, dict) and p in d:
            d = d[p]
        elif isinstance(d, list) and isinstance(p, int) and p < len(d):
            d = d[p]
        else:
            return default
    return d


def summarize_player(name, status, p):
    print(f"\n== player [{name}] HTTP {status}")
    ps = p.get("playabilityStatus", {})
    print(f"  playability       : {ps.get('status')} {('- ' + ps['reason']) if ps.get('reason') else ''}")
    print(f"  liveStreamability : {'yes' if 'liveStreamability' in ps else 'no'}")
    vd = p.get("videoDetails", {})
    for k in ("title", "author", "channelId", "isLive", "isLiveContent", "isPostLiveDvr", "isLowLatencyLiveStream", "latencyClass"):
        if k in vd:
            print(f"  videoDetails.{k:<23}: {vd[k]}")
    lbd = get(p, "microformat", "playerMicroformatRenderer", "liveBroadcastDetails")
    if lbd:
        print(f"  liveBroadcastDetails       : {json.dumps(lbd)}")
    cat = get(p, "microformat", "playerMicroformatRenderer", "category")
    if cat:
        print(f"  category                   : {cat}")
    hb = p.get("heartbeatParams")
    if hb:
        print(f"  heartbeatParams            : interval={hb.get('intervalMilliseconds')}ms "
              f"softFail={hb.get('softFailOnError')} serverData={'yes' if hb.get('heartbeatServerData') else 'no'}")
    sd = p.get("streamingData", {})
    if sd:
        print("  streamingData              : " + ", ".join(
            f"{k}={'yes' if k in sd else 'no'}" for k in ("hlsManifestUrl", "dashManifestUrl", "serverAbrStreamingUrl")
        ) + f", adaptiveFormats={len(sd.get('adaptiveFormats', []))}")
        for f in sd.get("adaptiveFormats", [])[:1]:
            q = urllib.parse.parse_qs(urllib.parse.urlparse(f.get("url", "")).query)
            if q:
                print(f"    first format url params  : {sorted(q)}")
            elif "signatureCipher" in f:
                print("    first format url         : signatureCipher (needs the JS player to decipher)")
    pt = p.get("playbackTracking", {})
    if pt:
        print("  playbackTracking urls      :")
        for k, v in pt.items():
            if isinstance(v, dict) and "baseUrl" in v:
                u = urllib.parse.urlparse(v["baseUrl"])
                params = sorted(urllib.parse.parse_qs(u.query))
                print(f"    {k:<28} {u.netloc}{u.path}  params={params}")
            else:
                print(f"    {k:<28} {json.dumps(v)[:80]}")
    hits = list(find_hints(p))
    print(f"  drops/rewards hints        : {len(hits)}")
    for path, val in hits[:15]:
        print(f"    {path} = {val}")


def find_hints(obj, path="$"):
    """Yield (json path, value) for keys or string values that look drops/rewards related."""
    if isinstance(obj, dict):
        for k, v in obj.items():
            if k in SKIP_KEYS:
                continue
            p = f"{path}.{k}"
            if HINTS.search(k):
                yield p, json.dumps(v)[:120]
            else:
                yield from find_hints(v, p)
    elif isinstance(obj, list):
        for i, v in enumerate(obj):
            yield from find_hints(v, f"{path}[{i}]")
    elif isinstance(obj, str) and HINTS.search(obj) and not obj.startswith("http"):
        yield path, obj[:120]


def summarize_next(status, n):
    print(f"\n== next [web] HTTP {status}")
    # Game card under the video ("richMetadataRenderer" with the game's title), if any.
    games = set()
    for path, val in walk(n):
        if path.endswith(".richMetadataRenderer.title.simpleText"):
            games.add(val)
    print(f"  game metadata cards : {sorted(games) or 'none found'}")
    hits = list(find_hints(n))
    print(f"  drops/rewards hints : {len(hits)}")
    for path, val in hits[:25]:
        print(f"    {path[-110:]} = {val}")


def walk(obj, path="$"):
    if isinstance(obj, dict):
        for k, v in obj.items():
            yield from walk(v, f"{path}.{k}")
    elif isinstance(obj, list):
        for i, v in enumerate(obj):
            yield from walk(v, f"{path}[{i}]")
    else:
        yield path, obj


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("target", help="11-char video id, or @handle (resolves the channel's current live)")
    ap.add_argument("--clients", default="web,mweb,tv,android_vr")
    ap.add_argument("--heartbeat", action="store_true", help="send ONE player/heartbeat call (web client)")
    ap.add_argument("--dump", help="directory where raw JSON responses are written")
    a = ap.parse_args()

    cfg = page_config(a.target)
    print(f"page: HTTP {cfg['status']} {cfg['final_url']}")
    print(f"  video id={cfg['video_id']} web clientVersion={cfg['client_version']} "
          f"visitorData={'yes' if cfg['visitor_data'] else 'no'} page playability={cfg['page_playability']}")
    vid = cfg["video_id"]
    if not vid:
        sys.exit("could not resolve a video id (channel not live?)")
    if a.dump:
        os.makedirs(a.dump, exist_ok=True)

    web_player = None
    for name in [c.strip() for c in a.clients.split(",") if c.strip()]:
        if name not in CLIENTS:
            print(f"unknown client {name}, skipped")
            continue
        time.sleep(PAUSE_S)
        body = {"videoId": vid, "contentCheckOk": True, "racyCheckOk": True}
        status, p = innertube("player", name, body, cfg)
        summarize_player(name, status, p)
        if name == "web":
            web_player = p
        if a.dump:
            with open(os.path.join(a.dump, f"player_{name}.json"), "w") as f:
                json.dump(p, f, indent=1)

    time.sleep(PAUSE_S)
    status, n = innertube("next", "web", {"videoId": vid}, cfg)
    summarize_next(status, n)
    if a.dump:
        with open(os.path.join(a.dump, "next_web.json"), "w") as f:
            json.dump(n, f, indent=1)

    if a.heartbeat:
        hb = (web_player or {}).get("heartbeatParams", {})
        time.sleep(PAUSE_S)
        body = {
            "videoId": vid,
            "heartbeatRequestParams": {"heartbeatChecks": ["HEARTBEAT_CHECK_TYPE_LIVE_STREAM_STATUS"]},
        }
        if hb.get("heartbeatServerData"):
            body["heartbeatServerData"] = hb["heartbeatServerData"]
        status, h = innertube("player/heartbeat", "web", body, cfg)
        print(f"\n== player/heartbeat [web] HTTP {status}")
        print("  " + json.dumps({k: h.get(k) for k in ("playabilityStatus", "heartbeatParams", "stopHeartbeat") if k in h})[:600])
        if a.dump:
            with open(os.path.join(a.dump, "heartbeat_web.json"), "w") as f:
                json.dump(h, f, indent=1)


if __name__ == "__main__":
    main()
