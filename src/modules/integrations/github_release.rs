// Jetporch
// Copyright (C) 2023 - Michael DeHaan <michael@michaeldehaan.net> + contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// at your option) any later version.
// 
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
// 
// You should have received a copy of the GNU General Public License
// long with this program.  If not, see <http://www.gnu.org/licenses/>.

use crate::tasks::*;
use crate::handle::handle::TaskHandle;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use semver::{Version, VersionReq};

const MODULE: &str = "GithubRelease";

#[derive(Deserialize, Debug)]
#[serde(deny_unknown_fields)]
pub struct GithubReleaseTask {
    pub name: Option<String>,
    pub repo: String,
    pub channel: Option<String>,
    pub version_filter: Option<String>,
    pub save: Option<String>,
    pub set: Option<String>,
    pub with: Option<PreLogicInput>,
    pub and: Option<PostLogicInput>,
}

struct GithubReleaseAction {
    pub repo: String,
    pub channel: String,
    pub version_filter: Option<String>,
    pub save_var: Option<String>,
    pub set_var: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct GithubRelease {
    tag_name: String,
    name: Option<String>,
    prerelease: bool,
    draft: bool,
    html_url: String,
    published_at: Option<String>,
    assets: Vec<GithubAsset>,
}

#[derive(Serialize, Deserialize, Debug)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
    size: i64,
}

impl IsTask for GithubReleaseTask {
    fn get_module(&self) -> String { String::from(MODULE) }
    fn get_name(&self) -> Option<String> { self.name.clone() }
    fn get_with(&self) -> Option<PreLogicInput> { self.with.clone() }

    fn evaluate(&self, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>, tm: TemplateMode) -> Result<EvaluatedTask, Arc<TaskResponse>> {
        let repo = handle.template.string(&request, tm, &String::from("repo"), &self.repo)?;
        let channel = match &self.channel {
            Some(c) => handle.template.string(&request, tm, &String::from("channel"), c)?,
            None => String::from("latest"), // Default to latest (stable) releases
        };
        let version_filter = match &self.version_filter {
            Some(v) => Some(handle.template.string(&request, tm, &String::from("version_filter"), v)?),
            None => None,
        };

        if self.save.is_some() && self.set.is_some() {
            return Err(handle.response.is_failed(&request, &String::from("Cannot use both 'save' and 'set' parameters")));
        }

        let save_var = match &self.save {
            Some(s) => Some(handle.template.string(&request, tm, &String::from("save"), s)?),
            None => None,
        };
        let set_var = match &self.set {
            Some(s) => Some(handle.template.string(&request, tm, &String::from("set"), s)?),
            None => None,
        };

        if save_var.is_none() && set_var.is_none() {
            return Err(handle.response.is_failed(&request, &String::from("Must specify either 'save' or 'set' parameter to store the version")));
        }

        return Ok(
            EvaluatedTask {
                action: Arc::new(GithubReleaseAction {
                    repo,
                    channel,
                    version_filter,
                    save_var,
                    set_var,
                }),
                with: Arc::new(PreLogicInput::template(&handle, &request, tm, &self.with)?),
                and: Arc::new(PostLogicInput::template(&handle, &request, tm, &self.and)?),
            }
        );
    }
}

impl IsAction for GithubReleaseAction {
    fn dispatch(&self, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>) -> Result<Arc<TaskResponse>, Arc<TaskResponse>> {
        match request.request_type {
            TaskRequestType::Query => {
                return Ok(handle.response.needs_passive(&request));
            },

            TaskRequestType::Passive => {
                let releases = self.fetch_github_releases(handle, request)?;
                let filtered_releases = self.filter_releases(&releases);
                
                if filtered_releases.is_empty() {
                    return Err(handle.response.is_failed(&request, &format!("No releases found for {} matching criteria", self.repo)));
                }

                let best_release = self.find_best_release(&filtered_releases, &self.channel)?;
                
                let version = best_release.tag_name.trim_start_matches('v');
                
                if let Some(ref var_name) = self.save_var {
                    let mut context = serde_yaml::Mapping::new();
                    context.insert(
                        serde_yaml::Value::String(var_name.clone()),
                        serde_yaml::Value::String(version.to_string())
                    );
                    handle.host.write().unwrap().set_variables(context);
                }
                
                if let Some(ref var_name) = self.set_var {
                    let mut mapping = serde_yaml::Mapping::new();
                    mapping.insert(
                        serde_yaml::Value::String(var_name.clone()),
                        serde_yaml::Value::String(version.to_string())
                    );
                    handle.host.write().unwrap().update_variables(mapping);
                }

                return Ok(handle.response.is_passive(&request));
            }

            _ => { return Err(handle.response.not_supported(request)); }
        }
    }
}

impl GithubReleaseAction {
    fn fetch_github_releases(&self, handle: &Arc<TaskHandle>, request: &Arc<TaskRequest>) -> Result<Vec<GithubRelease>, Arc<TaskResponse>> {
        let url = format!("https://api.github.com/repos/{}/releases", self.repo);
        
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| handle.response.is_failed(&request, &format!("Failed to create async runtime: {}", e)))?;
        
        rt.block_on(async {
            let client = reqwest::Client::builder()
                .user_agent("Jetpack/0.1")
                .build()
                .map_err(|e| handle.response.is_failed(&request, &format!("Failed to create HTTP client: {}", e)))?;
            
            let response = client.get(&url)
                .send()
                .await
                .map_err(|e| handle.response.is_failed(&request, &format!("Failed to fetch releases from GitHub: {}", e)))?;
            
            if !response.status().is_success() {
                let status = response.status();
                let text = response.text().await.unwrap_or_default();
                return Err(handle.response.is_failed(&request, &format!("GitHub API returned status {}: {}", status, text)));
            }
            
            let releases: Vec<GithubRelease> = response.json()
                .await
                .map_err(|e| handle.response.is_failed(&request, &format!("Failed to parse GitHub response: {}", e)))?;
            
            Ok(releases)
        })
    }

    fn filter_releases<'a>(&self, releases: &'a [GithubRelease]) -> Vec<&'a GithubRelease> {
        let version_filtered: Vec<&GithubRelease> = releases.iter()
            .filter(|r| !r.draft)
            .filter(|r| {
                if let Some(ref filter) = self.version_filter {
                    self.matches_version_filter(&r.tag_name, filter)
                } else {
                    true
                }
            })
            .collect();

        match self.channel.as_str() {
            "stable" | "latest" => {
                // Only stable releases (stable and latest are synonyms)
                version_filtered.into_iter()
                    .filter(|r| !r.prerelease)
                    .collect()
            },
            "unstable" => {
                // Only prereleases (RCs, betas, alphas)
                version_filtered.into_iter()
                    .filter(|r| r.prerelease)
                    .collect()
            },
            "prerelease" => {
                // Smart mode: get the absolute latest, but prefer stable over RC of the same version
                // e.g., prefer 0.36.0 over 0.36.0-rc2, but prefer 0.36.0-rc2 over 0.35.0
                version_filtered
            },
            "any" => {
                // Return all non-draft releases
                version_filtered
            },
            _ => {
                // Default to stable for unknown channel values
                version_filtered.into_iter()
                    .filter(|r| !r.prerelease)
                    .collect()
            }
        }
    }

    fn matches_version_filter(&self, tag: &str, filter: &str) -> bool {
        let version_str = tag.trim_start_matches('v');
        
        if let Ok(version) = Version::parse(version_str) {
            if let Ok(req) = VersionReq::parse(filter) {
                return req.matches(&version);
            }
        }
        
        false
    }

    fn find_best_release<'a>(&self, releases: &[&'a GithubRelease], _channel: &str) -> Result<&'a GithubRelease, Arc<TaskResponse>> {
        let mut sorted_releases: Vec<&GithubRelease> = releases.to_vec();
        
        sorted_releases.sort_by(|a, b| {
            let a_version = Version::parse(a.tag_name.trim_start_matches('v'));
            let b_version = Version::parse(b.tag_name.trim_start_matches('v'));
            
            match (a_version, b_version) {
                (Ok(a_v), Ok(b_v)) => {
                    // Standard semver comparison (0.36.0-rc2 < 0.36.0)
                    b_v.cmp(&a_v)
                },
                (Ok(_), Err(_)) => std::cmp::Ordering::Less,
                (Err(_), Ok(_)) => std::cmp::Ordering::Greater,
                (Err(_), Err(_)) => b.tag_name.cmp(&a.tag_name),
            }
        });
        
        sorted_releases.first().copied().ok_or_else(|| {
            Arc::from(TaskResponse {
                status: TaskStatus::Failed,
                changes: Vec::new(),
                msg: Some(String::from("No matching releases found")),
                command_result: Arc::new(None),
                with: Arc::new(None),
                and: Arc::new(None),
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_version_filter_matching() {
        let action = GithubReleaseAction {
            repo: String::from("test/repo"),
            channel: String::from("stable"),
            version_filter: Some(String::from("~2.1.0")),
            save_var: Some(String::from("version")),
            set_var: None,
        };
        
        // Test exact version match
        assert!(action.matches_version_filter("v2.1.0", "~2.1.0"));
        assert!(action.matches_version_filter("v2.1.5", "~2.1.0"));
        assert!(!action.matches_version_filter("v2.2.0", "~2.1.0"));
        
        // Test version range
        assert!(action.matches_version_filter("v2.1.0", ">=2.0.0, <3.0.0"));
        assert!(action.matches_version_filter("v2.5.0", ">=2.0.0, <3.0.0"));
        assert!(!action.matches_version_filter("v3.0.0", ">=2.0.0, <3.0.0"));
        
        // Test caret requirements
        assert!(action.matches_version_filter("v1.2.3", "^1.2.3"));
        assert!(action.matches_version_filter("v1.2.9", "^1.2.3"));
        assert!(!action.matches_version_filter("v2.0.0", "^1.2.3"));
    }
    
    #[test]
    fn test_release_filtering() {
        let action = GithubReleaseAction {
            repo: String::from("test/repo"),
            channel: String::from("stable"),
            version_filter: None,
            save_var: Some(String::from("version")),
            set_var: None,
        };
        
        let releases = vec![
            GithubRelease {
                tag_name: String::from("v2.0.0"),
                name: Some(String::from("Release 2.0.0")),
                prerelease: false,
                draft: false,
                html_url: String::from("https://github.com/test/repo/releases/tag/v2.0.0"),
                published_at: Some(String::from("2024-01-01T00:00:00Z")),
                assets: vec![],
            },
            GithubRelease {
                tag_name: String::from("v2.1.0-beta"),
                name: Some(String::from("Beta 2.1.0")),
                prerelease: true,
                draft: false,
                html_url: String::from("https://github.com/test/repo/releases/tag/v2.1.0-beta"),
                published_at: Some(String::from("2024-01-02T00:00:00Z")),
                assets: vec![],
            },
            GithubRelease {
                tag_name: String::from("v1.9.0"),
                name: Some(String::from("Release 1.9.0")),
                prerelease: false,
                draft: true,
                html_url: String::from("https://github.com/test/repo/releases/tag/v1.9.0"),
                published_at: Some(String::from("2023-12-01T00:00:00Z")),
                assets: vec![],
            },
        ];
        
        let filtered = action.filter_releases(&releases);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].tag_name, "v2.0.0");
        
        // Test unstable channel (only prereleases)
        let unstable_action = GithubReleaseAction {
            repo: String::from("test/repo"),
            channel: String::from("unstable"),
            version_filter: None,
            save_var: Some(String::from("version")),
            set_var: None,
        };
        
        let filtered_unstable = unstable_action.filter_releases(&releases);
        assert_eq!(filtered_unstable.len(), 1);
        assert_eq!(filtered_unstable[0].tag_name, "v2.1.0-beta");
        
        // Test prerelease channel - returns all releases, sorting will prefer stable of same version
        let prerelease_action = GithubReleaseAction {
            repo: String::from("test/repo"),
            channel: String::from("prerelease"),
            version_filter: None,
            save_var: Some(String::from("version")),
            set_var: None,
        };
        
        let filtered_prerelease = prerelease_action.filter_releases(&releases);
        assert_eq!(filtered_prerelease.len(), 2); // Gets both stable and prerelease
        
        // Test any channel
        let any_action = GithubReleaseAction {
            repo: String::from("test/repo"),
            channel: String::from("any"),
            version_filter: None,
            save_var: Some(String::from("version")),
            set_var: None,
        };
        
        let filtered_any = any_action.filter_releases(&releases);
        assert_eq!(filtered_any.len(), 2); // Gets both stable and prerelease
        
        // Test latest channel (synonym for stable)
        let latest_action = GithubReleaseAction {
            repo: String::from("test/repo"),
            channel: String::from("latest"),
            version_filter: None,
            save_var: Some(String::from("version")),
            set_var: None,
        };
        
        let filtered_latest = latest_action.filter_releases(&releases);
        assert_eq!(filtered_latest.len(), 1);
        assert_eq!(filtered_latest[0].tag_name, "v2.0.0"); // Only stable
    }
    
    #[test]
    fn test_best_release_selection() {
        let action = GithubReleaseAction {
            repo: String::from("test/repo"),
            channel: String::from("stable"),
            version_filter: None,
            save_var: Some(String::from("version")),
            set_var: None,
        };
        
        let release1 = GithubRelease {
            tag_name: String::from("v1.2.3"),
            name: Some(String::from("Release 1.2.3")),
            prerelease: false,
            draft: false,
            html_url: String::from("https://github.com/test/repo/releases/tag/v1.2.3"),
            published_at: Some(String::from("2024-01-01T00:00:00Z")),
            assets: vec![],
        };
        
        let release2 = GithubRelease {
            tag_name: String::from("v2.0.0"),
            name: Some(String::from("Release 2.0.0")),
            prerelease: false,
            draft: false,
            html_url: String::from("https://github.com/test/repo/releases/tag/v2.0.0"),
            published_at: Some(String::from("2024-01-02T00:00:00Z")),
            assets: vec![],
        };
        
        let release3 = GithubRelease {
            tag_name: String::from("v1.9.9"),
            name: Some(String::from("Release 1.9.9")),
            prerelease: false,
            draft: false,
            html_url: String::from("https://github.com/test/repo/releases/tag/v1.9.9"),
            published_at: Some(String::from("2024-01-03T00:00:00Z")),
            assets: vec![],
        };
        
        let releases = vec![&release1, &release2, &release3];
        let best = action.find_best_release(&releases, "stable").unwrap();
        assert_eq!(best.tag_name, "v2.0.0");
    }
    
    #[test]
    fn test_prerelease_channel_smart_selection() {
        let action = GithubReleaseAction {
            repo: String::from("test/repo"),
            channel: String::from("prerelease"),
            version_filter: None,
            save_var: Some(String::from("version")),
            set_var: None,
        };
        
        // Scenario 1: v0.36.0-rc2 vs v0.35.0 - should pick the newer RC
        let releases1 = vec![
            GithubRelease {
                tag_name: String::from("v0.36.0-rc2"),
                name: Some(String::from("Release Candidate")),
                prerelease: true,
                draft: false,
                html_url: String::from(""),
                published_at: Some(String::from("2024-01-02T00:00:00Z")),
                assets: vec![],
            },
            GithubRelease {
                tag_name: String::from("v0.35.0"),
                name: Some(String::from("Stable Release")),
                prerelease: false,
                draft: false,
                html_url: String::from(""),
                published_at: Some(String::from("2024-01-01T00:00:00Z")),
                assets: vec![],
            },
        ];
        
        let refs1: Vec<&GithubRelease> = releases1.iter().collect();
        let best1 = action.find_best_release(&refs1, "prerelease").unwrap();
        assert_eq!(best1.tag_name, "v0.36.0-rc2"); // Newer version wins
        
        // Scenario 2: v0.36.0 vs v0.36.0-rc2 - should pick the stable
        let releases2 = vec![
            GithubRelease {
                tag_name: String::from("v0.36.0"),
                name: Some(String::from("Stable Release")),
                prerelease: false,
                draft: false,
                html_url: String::from(""),
                published_at: Some(String::from("2024-01-03T00:00:00Z")),
                assets: vec![],
            },
            GithubRelease {
                tag_name: String::from("v0.36.0-rc2"),
                name: Some(String::from("Release Candidate")),
                prerelease: true,
                draft: false,
                html_url: String::from(""),
                published_at: Some(String::from("2024-01-02T00:00:00Z")),
                assets: vec![],
            },
        ];
        
        let refs2: Vec<&GithubRelease> = releases2.iter().collect();
        let best2 = action.find_best_release(&refs2, "prerelease").unwrap();
        assert_eq!(best2.tag_name, "v0.36.0"); // Stable of same version wins
    }
    
    #[test]
    fn test_task_deserialization() {
        let yaml = r#"
            repo: "ipfs/kubo"
            channel: "stable"
            save: "ipfs_version"
        "#;
        
        let task: Result<GithubReleaseTask, _> = serde_yaml::from_str(yaml);
        assert!(task.is_ok());
        
        let task = task.unwrap();
        assert_eq!(task.repo, "ipfs/kubo");
        assert_eq!(task.channel, Some(String::from("stable")));
        assert_eq!(task.save, Some(String::from("ipfs_version")));
        assert!(task.set.is_none());
    }
    
    #[test]
    fn test_task_with_version_filter() {
        let yaml = r#"
            repo: "nodejs/node"
            channel: "stable"
            version_filter: "^18.0.0"
            set: "node_version"
        "#;
        
        let task: Result<GithubReleaseTask, _> = serde_yaml::from_str(yaml);
        assert!(task.is_ok());
        
        let task = task.unwrap();
        assert_eq!(task.repo, "nodejs/node");
        assert_eq!(task.version_filter, Some(String::from("^18.0.0")));
        assert_eq!(task.set, Some(String::from("node_version")));
    }
}