//! IGDB (Twitch) client: search, metadata and artwork download (port of igdb.ts).

use crate::config::Settings;
use crate::log;
use nostalgia_shared::{GameMetadata, Video, iso_from_millis, platform_info};
use serde::Serialize;
use serde_json::Value;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const IMG: &str = "https://images.igdb.com/igdb/image/upload";
const MAX_ARTWORKS: usize = 6;
const MAX_SCREENSHOTS: usize = 8;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IgdbSearchHit {
    pub igdb_id: i64,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<i64>,
    pub platforms: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover_url: Option<String>,
}

struct State {
    token: Option<(String, Instant)>,
    last_request: Option<Instant>,
}

pub struct Igdb {
    settings: Arc<Settings>,
    http: reqwest::Client,
    /// Serialises requests (IGDB allows 4 requests / second).
    state: Mutex<State>,
}

fn year_of(secs: i64) -> i64 {
    iso_from_millis(secs * 1000)[..4].parse().unwrap_or(0)
}

fn names(v: &Value, k: &str) -> Vec<String> {
    v.get(k).and_then(|a| a.as_array()).map(|a| a.iter().filter_map(|x| x.get("name")?.as_str().map(String::from)).collect()).unwrap_or_default()
}

fn round(v: Option<&Value>) -> Option<i64> {
    v.and_then(|x| x.as_f64()).map(|f| f.round() as i64)
}

impl Igdb {
    pub fn new(settings: Arc<Settings>, http: reqwest::Client) -> Igdb {
        Igdb { settings, http, state: Mutex::new(State { token: None, last_request: None }) }
    }

    pub fn configured(&self) -> bool {
        !self.settings.igdb_client_id.is_empty() && !self.settings.igdb_client_secret.is_empty()
    }

    async fn token(&self, st: &mut State) -> anyhow::Result<String> {
        if let Some((t, exp)) = &st.token
            && *exp > Instant::now() + Duration::from_secs(60)
        {
            return Ok(t.clone());
        }
        let res = self
            .http
            .post("https://id.twitch.tv/oauth2/token")
            .query(&[
                ("client_id", self.settings.igdb_client_id.as_str()),
                ("client_secret", self.settings.igdb_client_secret.as_str()),
                ("grant_type", "client_credentials"),
            ])
            .send()
            .await?;
        if !res.status().is_success() {
            let code = res.status().as_u16();
            anyhow::bail!("IGDB authentication failed ({code}): {}", res.text().await.unwrap_or_default());
        }
        let j: Value = res.json().await?;
        let token = j.get("access_token").and_then(|v| v.as_str()).unwrap_or_default().to_string();
        let expires = j.get("expires_in").and_then(|v| v.as_u64()).unwrap_or(3600);
        st.token = Some((token.clone(), Instant::now() + Duration::from_secs(expires)));
        Ok(token)
    }

    pub async fn query(&self, endpoint: &str, body: String) -> anyhow::Result<Value> {
        let mut st = self.state.lock().await;
        if !self.configured() {
            anyhow::bail!("IGDB credentials are not configured in the settings file");
        }
        if let Some(last) = st.last_request {
            let wait = Duration::from_millis(280).saturating_sub(last.elapsed());
            if !wait.is_zero() {
                tokio::time::sleep(wait).await;
            }
        }
        st.last_request = Some(Instant::now());
        let mut attempt = 0;
        loop {
            let token = self.token(&mut st).await?;
            let res = self
                .http
                .post(format!("https://api.igdb.com/v4/{endpoint}"))
                .header("Client-ID", &self.settings.igdb_client_id)
                .header("Authorization", format!("Bearer {token}"))
                .header("Accept", "application/json")
                .body(body.clone())
                .send()
                .await?;
            let status = res.status().as_u16();
            if status == 429 && attempt < 5 {
                tokio::time::sleep(Duration::from_millis(1000 * (attempt + 1))).await;
                attempt += 1;
                continue;
            }
            if status == 401 && attempt < 1 {
                st.token = None;
                attempt += 1;
                continue;
            }
            if !res.status().is_success() {
                anyhow::bail!("IGDB {endpoint} failed ({status}): {}", res.text().await.unwrap_or_default());
            }
            return Ok(res.json().await?);
        }
    }

    pub async fn search(&self, q: &str, platform: Option<&str>) -> anyhow::Result<Vec<IgdbSearchHit>> {
        let term = q.replace('"', "");
        let term = term.trim();
        if term.is_empty() {
            return Ok(vec![]);
        }
        let id_match = term.chars().all(|c| c.is_ascii_digit());
        let pid = platform.and_then(|p| platform_info(p).igdb);
        let fields = "fields name,first_release_date,platforms.name,cover.image_id;";
        let body = if id_match {
            format!("{fields} where id = {term};")
        } else {
            format!("search \"{term}\"; {fields} {} limit 25;", pid.map(|p| format!("where platforms = ({p});")).unwrap_or_default())
        };
        let mut rows = self.query("games", body).await?;
        if rows.as_array().is_none_or(|a| a.is_empty()) && pid.is_some() && !id_match {
            rows = self.query("games", format!("search \"{term}\"; {fields} limit 25;")).await?;
        }
        Ok(rows
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|g| IgdbSearchHit {
                        igdb_id: g.get("id").and_then(|v| v.as_i64()).unwrap_or(0),
                        name: g.get("name").and_then(|v| v.as_str()).unwrap_or_default().to_string(),
                        year: g.get("first_release_date").and_then(|v| v.as_i64()).filter(|s| *s != 0).map(year_of),
                        platforms: names(g, "platforms"),
                        cover_url: g
                            .get("cover")
                            .and_then(|c| c.get("image_id"))
                            .and_then(|v| v.as_str())
                            .map(|id| format!("{IMG}/t_cover_small/{id}.jpg")),
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    pub async fn fetch_game(&self, igdb_id: i64) -> anyhow::Result<Value> {
        let rows = self
            .query(
                "games",
                format!(
                    "fields name,slug,summary,storyline,url,first_release_date,rating,rating_count,total_rating,total_rating_count,aggregated_rating,
        genres.name,themes.name,game_modes.name,player_perspectives.name,franchises.name,collection.name,platforms.name,
        involved_companies.company.name,involved_companies.developer,involved_companies.publisher,
        cover.image_id,artworks.image_id,screenshots.image_id,videos.name,videos.video_id;
       where id = {igdb_id};"
                ),
            )
            .await?;
        rows.as_array().and_then(|a| a.first().cloned()).ok_or_else(|| anyhow::anyhow!("IGDB game {igdb_id} not found"))
    }

    /// Download metadata + cover + artworks + screenshots of the game in `game_dir` into `dir`
    /// (its folder in the metadata directory). Existing files are overwritten in place (never deleted).
    pub async fn download_into(&self, game_dir: &Path, dir: &Path, igdb_id: i64) -> anyhow::Result<GameMetadata> {
        let g = self.fetch_game(igdb_id).await?;
        // Never recreate a game folder that was renamed or moved while the download was queued.
        if !game_dir.exists() {
            anyhow::bail!("Game folder no longer exists: {}", game_dir.display());
        }
        tokio::fs::create_dir_all(dir).await?;

        let image_id = |v: &Value| v.get("image_id").and_then(|x| x.as_str()).map(String::from);
        let cover = match g.get("cover").and_then(image_id) {
            Some(id) => self.save_image(&format!("{IMG}/t_cover_big_2x/{id}.jpg"), &dir.join("cover.jpg")).await,
            None => false,
        };
        let mut artworks = Vec::new();
        for (i, a) in g.get("artworks").and_then(|v| v.as_array()).cloned().unwrap_or_default().iter().take(MAX_ARTWORKS).enumerate() {
            let name = format!("artwork-{:02}.jpg", i + 1);
            if let Some(id) = image_id(a)
                && self.save_image(&format!("{IMG}/t_1080p/{id}.jpg"), &dir.join(&name)).await
            {
                artworks.push(name);
            }
        }
        let mut screenshots = Vec::new();
        for (i, s) in g.get("screenshots").and_then(|v| v.as_array()).cloned().unwrap_or_default().iter().take(MAX_SCREENSHOTS).enumerate() {
            let name = format!("screenshot-{:02}.jpg", i + 1);
            if let Some(id) = image_id(s)
                && self.save_image(&format!("{IMG}/t_1080p/{id}.jpg"), &dir.join(&name)).await
            {
                screenshots.push(name);
            }
        }

        let companies = g.get("involved_companies").and_then(|v| v.as_array()).cloned().unwrap_or_default();
        let company = |flag: &str| -> Vec<String> {
            companies
                .iter()
                .filter(|c| c.get(flag).and_then(|v| v.as_bool()).unwrap_or(false))
                .filter_map(|c| c.get("company")?.get("name")?.as_str().map(String::from))
                .collect()
        };
        let mut franchises = names(&g, "franchises");
        if let Some(c) = g.get("collection").and_then(|c| c.get("name")).and_then(|v| v.as_str()) {
            franchises.push(c.to_string());
        }
        let mut seen = Vec::new();
        franchises.retain(|f| {
            if seen.contains(f) {
                false
            } else {
                seen.push(f.clone());
                true
            }
        });
        let release = g.get("first_release_date").and_then(|v| v.as_i64()).filter(|s| *s != 0);
        let s = |k: &str| g.get(k).and_then(|v| v.as_str()).map(String::from);
        let meta = GameMetadata {
            igdb_id: g.get("id").and_then(|v| v.as_i64()).unwrap_or(igdb_id),
            name: s("name").unwrap_or_default(),
            slug: s("slug"),
            summary: s("summary"),
            storyline: s("storyline"),
            url: s("url"),
            release_date: release.map(|r| iso_from_millis(r * 1000)[..10].to_string()),
            year: release.map(year_of),
            rating: round(g.get("total_rating")).or_else(|| round(g.get("rating"))),
            rating_count: g.get("total_rating_count").or_else(|| g.get("rating_count")).and_then(|v| v.as_i64()),
            aggregated_rating: round(g.get("aggregated_rating")),
            genres: names(&g, "genres"),
            themes: names(&g, "themes"),
            game_modes: names(&g, "game_modes"),
            perspectives: names(&g, "player_perspectives"),
            developers: company("developer"),
            publishers: company("publisher"),
            franchises,
            platforms: names(&g, "platforms"),
            videos: g
                .get("videos")
                .and_then(|v| v.as_array())
                .map(|a| {
                    a.iter()
                        .filter_map(|v| {
                            Some(Video { name: v.get("name").and_then(|n| n.as_str()).map(String::from), youtube_id: v.get("video_id")?.as_str()?.to_string() })
                        })
                        .collect()
                })
                .unwrap_or_default(),
            cover: cover.then(|| "cover.jpg".to_string()),
            artworks,
            screenshots,
            fetched_at: nostalgia_shared::iso_now(),
        };
        tokio::fs::write(dir.join("igdb-raw.json"), serde_json::to_string_pretty(&g)?).await?;
        tokio::fs::write(dir.join("metadata.json"), serde_json::to_string_pretty(&meta)?).await?;
        Ok(meta)
    }

    async fn save_image(&self, url: &str, file: &Path) -> bool {
        let res: anyhow::Result<()> = async {
            let res = self.http.get(url).send().await?;
            if !res.status().is_success() {
                anyhow::bail!("HTTP {}", res.status().as_u16());
            }
            let buf = res.bytes().await?;
            let tmp = file.with_extension("jpg.tmp");
            tokio::fs::write(&tmp, &buf).await?;
            tokio::fs::rename(&tmp, file).await?;
            Ok(())
        }
        .await;
        match res {
            Ok(()) => true,
            Err(e) => {
                log::warn(format!("Image download failed {url}: {e}"));
                file.exists()
            }
        }
    }
}
