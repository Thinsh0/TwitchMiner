import requests
import uuid
import logging
from utils.config import ConfigManager
import re
import base64
import json

class TwitchGQL:
    def __init__(self, config_manager: ConfigManager):
        self.config = config_manager
        self.client_id = self.config.get("client_id")
        self.user_agent = "Dalvik/2.1.0 (Linux; U; Android 16; SM-S911B Build/TP1A.220624.014) tv.twitch.android.app/25.3.0/2503006"
        self.session = requests.Session()
        self.session.headers.update({"User-Agent": self.user_agent})

    def _get_headers(self):
        token = self.config.get_auth_token()
        return {
            "Client-ID": self.client_id,
            "Authorization": f"OAuth {token}",
            "Content-Type": "application/json",
            "User-Agent": self.user_agent,
            "X-Device-Id": self.get_device_id()
        }
    
    def get_device_id(self):
        # Generate a random decent device ID if not existing, or mock one
        # Twitch usually wants a random string
        return "ecd" + uuid.uuid4().hex[:29]

    def post(self, operation_name, query, variables=None):
        url = "https://gql.twitch.tv/gql"
        payload = {
            "operationName": operation_name,
            "query": query,
            "variables": variables or {}
        }
        
        try:
            response = requests.post(url, json=payload, headers=self._get_headers())
            response.raise_for_status()
            return response.json()
        except Exception as e:
            print(f"GQL Error [{operation_name}]: {e}")
            return None

    def get_inventory(self):
        # Use Persisted Query Hash from TwitchDropsMiner to ensure correct fields
        sha256 = "d86775d0ef16a63a33ad52e80eaff963b2d5b72fada7c991504a57496e1d8e4b"
        payload = {
            "operationName": "Inventory",
            "extensions": {
                "persistedQuery": {
                    "version": 1,
                    "sha256Hash": sha256
                }
            },
            "variables": {
                "fetchRewardCampaigns": False
            }
        }
        
        try:
            # We use a custom POST here because the generic 'post' method expects a query string
            # and constructs the payload differently.
            url = "https://gql.twitch.tv/gql"
            response = self.session.post(url, json=payload, headers=self._get_headers())
            response.raise_for_status()
            return response.json()
        except Exception as e:
            print(f"GQL Error [Inventory]: {e}")
            return None

    def get_drops_campaigns(self):
        # ViewerDropsDashboard requires Integrity Token which we don't have.
        # Fallback to using Inventory which contains 'dropCampaignsInProgress'.
        # This is a filtered list but better than an error.
        print("[API] Fetching Campaigns via Inventory (Fallback)...")
        return self.get_inventory()

    def claim_drop(self, drop_id):
        # DropsPage_ClaimDropRewards
        sha256 = "a455deea71bdc9015b78eb49f4acfbce8baa7ccbedd28e549bb025bd0f751930"
        payload = {
            "operationName": "DropsPage_ClaimDropRewards",
            "extensions": {
                "persistedQuery": {
                    "version": 1,
                    "sha256Hash": sha256
                }
            },
            "variables": {
                "input": {
                    "dropInstanceID": drop_id
                }
            }
        }
        try:
             url = "https://gql.twitch.tv/gql"
             response = self.session.post(url, json=payload, headers=self._get_headers())
             response.raise_for_status()
             return response.json()
        except Exception as e:
             print(f"[API] Error claiming drop {drop_id}: {e}")
             return None
             
    def claim_drop_legacy(self, drop_id):
        query = """
        mutation ClaimDrop($input: ClaimDropInput!) {
            claimDrop(input: $input) {
                drop {
                    id
                    status
                }
                errors {
                    code
                    message
                }
            }
        }
        """
        variables = {
            "input": {
                "dropInstanceID": drop_id
            }
        }
        return self.post("ClaimDrop", query, variables)

    def get_live_streams(self, game_id):
        query = """
        query GetLiveStreams($gameID: ID!) {
            game(id: $gameID) {
                streams(first: 20, options: {sort: VIEWER_COUNT}) {
                    edges {
                        node {
                            id
                            broadcaster {
                                id
                                login
                                displayName
                                profileImageURL(width: 70)
                            }
                            viewersCount
                            tags {
                                id
                                localizedName
                            }
                            freeformTags {
                                name
                            }
                        }
                    }
                }
            }
        }
        """
        variables = {"gameID": game_id}
        # Note: Generic post() cannot handle complex persists easily, so we use ad-hoc query
        # But wait, self.post() handles standard queries fine!
        return self.post("GetLiveStreams", query, variables)


    def get_stream_info(self, channel_login):
        query = """
        query GetStreamInfo($login: String!) {
            user(login: $login) {
                id
                profileImageURL(width: 70)
                stream {
                    id
                    game {
                        id
                        name
                    }
                    viewersCount
                }
            }
        }
        """
        variables = {"login": channel_login}
        return self.post("GetStreamInfo", query, variables)
    def get_playback_access_token(self, channel_login):
        query = """
        query PlaybackAccessToken($login: String!, $template: String) {
            streamPlaybackAccessToken(channelName: $login, params: {platform: "web", playerBackend: "mediaplayer", playerType: "site"}) {
                value
                signature
            }
        }
        """
        variables = {"login": channel_login, "template": None}
        return self.post("PlaybackAccessToken", query, variables)   

    def search_games(self, query_text):
        query = """
        query SearchFor($query: String!) {
            searchFor(userQuery: $query, platform: "web", target: {index: GAME}) {
                games {
                    edges {
                        item {
                            ... on Game {
                                id
                                displayName
                                name
                                boxArtURL(width: 52, height: 72)
                            }
                        }
                    }
                }
            }
        }
        """
        variables = {"query": query_text}
        response = self.post("SearchFor", query, variables)
        return response

    def get_slug(self, game_name):
        """Converts game name to slug (Reference: TwitchDropsMiner/utils.py)"""
        # 1. Lowercase
        slug_text = game_name.lower()
        # 2. Remove apostrophes
        slug_text = re.sub(r"\'", '', slug_text)
        # 3. Replace non-word chars with hyphens
        slug_text = re.sub(r"\W+", '-', slug_text)
        # 4. Strip duplicate hyphens
        slug_text = re.sub(r"-{2,}", '-', slug_text).strip('-')
        return slug_text

    def get_live_streams_by_slug(self, game_slug):
        # Hash for DirectoryPage_Game
        sha256 = "98a996c3c3ebb1ba4fd65d6671c6028d7ee8d615cb540b0731b3db2a911d3649"
        payload = {
            "operationName": "DirectoryPage_Game",
            "extensions": {
                "persistedQuery": {
                    "version": 1,
                    "sha256Hash": sha256
                }
            },
            "variables": {
                "limit": 20,
                "slug": game_slug,
                "sortTypeIsRecency": False,
                "imageWidth": 50,
                "includeCostreaming": False,
                "options": {
                    "broadcasterLanguages": [],
                    "freeformTags": None,
                    "includeRestricted": ["SUB_ONLY_LIVE"],
                    "recommendationsContext": {"platform": "web"},
                    "sort": "RELEVANCE",
                    "systemFilters": [],
                    "tags": []
                }
            }
        }
        try:
             url = "https://gql.twitch.tv/gql"
             response = self.session.post(url, json=payload, headers=self._get_headers())
             response.raise_for_status()
             data = response.json()
             print(f"[API DEBUG] Raw Stream Data for {game_slug}: {data}") 
             return data
        except Exception as e:
             print(f"[API] Error fetching live streams for {game_slug}: {e}")
             return None

    def get_available_drops(self, channel_id):
        # DropsHighlightService_AvailableDrops
        sha256 = "9a62a09bce5b53e26e64a671e530bc599cb6aab1e5ba3cbd5d85966d3940716f"
        payload = {
            "operationName": "DropsHighlightService_AvailableDrops",
            "extensions": {
                "persistedQuery": {
                    "version": 1,
                    "sha256Hash": sha256
                }
            },
            "variables": {
                "channelID": str(channel_id)
            }
        }
        try:
             # My self.post() wrapper expects (op_name, query, vars)... 
             # But here I constructed full payload for persisted. 
             # Let's just use session.post directly to be safe and consistent with get_live_streams
             
             url = "https://gql.twitch.tv/gql"
             response = self.session.post(url, json=payload, headers=self._get_headers())
             response.raise_for_status()
             data = response.json()
             print(f"[API DEBUG] Available Drops for {channel_id}: {data}")
             return data
        except Exception as e:
             print(f"[API] Error fetching available drops for {channel_id}: {e}")
             return None

    def get_user_info(self):
         query = """
         query UserInfo {
             currentUser {
                 id
                 login
                 displayName
                 profileImageURL(width: 50)
             }
         }
         """
         return self.post("UserInfo", query)

    def get_spade_url(self, streamer_login):
        """Extracts the Spade URL from the streamer's channel page."""
        url = f"https://www.twitch.tv/{streamer_login}"
        try:
            # Step 1: Get Channel Page
            # We must use a proper User-Agent to get the page content
            headers = {"User-Agent": self.user_agent} 
            resp = self.session.get(url, headers=headers)
            if resp.status_code != 200:
                print(f"[API] Failed to fetch channel page: {resp.status_code}")
                return None
                
            page_html = resp.text
            
            # Regex patterns from TwitchDropsMiner
            SPADE_PATTERN = r'"beacon_?url": ?"(https://video-edge-[.\w\-/]+\.ts(?:\?allow_stream=true)?)"'
            match = re.search(SPADE_PATTERN, page_html, re.I)
            
            if match:
                return match.group(1)
                
            # Fallback: Look for settings JS if not in main HTML (rare now but possible)
            SETTINGS_PATTERN = r'src="(https://[\w.]+/config/settings\.[0-9a-f]{32}\.js)"'
            match_settings = re.search(SETTINGS_PATTERN, page_html, re.I)
            if match_settings:
                settings_url = match_settings.group(1)
                resp_js = self.session.get(settings_url, headers=headers)
                match_js = re.search(SPADE_PATTERN, resp_js.text, re.I)
                if match_js:
                    return match_js.group(1)
            
            print(f"[API] Could not find Spade URL for {streamer_login}")
            return None
            
        except Exception as e:
            print(f"[API] Error getting Spade URL: {e}")
            return None

    def send_spade_minute_watched(self, spade_url, payload_data):
        """Sends the minute-watched heartbeat to the Spade URL."""
        if not spade_url: return False
        
        # TwitchDropsMiner minifies json and base64 encodes it
        # Payload structure: {"data": base64_encoded_str}
        try:
            json_str = json.dumps(payload_data, separators=(',', ':'))
            b64_data = base64.b64encode(json_str.encode('utf-8')).decode('utf-8')
            final_payload = {"data": b64_data}
            
            # POST request
            # Using plain requests for now as we are synchronous in this thread
            resp = self.session.post(spade_url, data=final_payload)
            # print(f"[API DEBUG] Spade Heartbeat Status: {resp.status_code}")
            return resp.status_code == 204
        except Exception as e:
            print(f"[API] Error sending Spade heartbeat: {e}")
            return False

    
    def claim_community_points(self, channel_id, claim_id):
        query = """
        mutation ClaimCommunityPoints($input: ClaimCommunityPointsInput!) {
            claimCommunityPoints(input: $input) {
                pointsEarned
                claim {
                    id
                    reason
                }
            }
        }
        """
        variables = {"input": {"channelID": channel_id, "claimID": claim_id}}
        return self.post("ClaimCommunityPoints", query, variables)
    

    
    def get_channel_points_context(self, channel_login):
        # We need the channel's ID properly contexted
        query = """
        query GetChannelPointsContext($login: String!) {
            channel(name: $login) {
                id
                displayName
                self {
                    communityPoints {
                        balance
                    }
                }
            }
        }
        """
        variables = {"login": channel_login}
        return self.post("GetChannelPointsContext", query, variables)
     
    
    def claim_community_points(self, channel_id, claim_id):
        pass

    def get_streak_info(self, channel_id):
        # Trying to fetch streak via channel user context
        # This is often not public API, but we can try to look at 'stream' -> 'viewers' or similar
        # Actually proper streaks are hard. Let's try 'relationship' or similar.
        # Fallback: We just return a placeholder or 0 if we can't find it.
        return 0
