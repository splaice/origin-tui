use std::path::Path;

use crate::app::{Job, Msg};
use crate::origin;

pub fn execute(bin: &Path, job: Job) -> Msg {
    match job {
        Job::Auth => Msg::Auth(origin::auth_status(bin)),
        Job::Namespaces => Msg::Namespaces(origin::namespaces(bin)),
        Job::RepoList { generation } => Msg::RepoList {
            generation,
            result: origin::repo_list(bin),
        },
        Job::Enrich { owner } => Msg::Enrich {
            result: origin::list_owner_repos(bin, &owner),
            owner,
        },
        Job::Detail {
            full_name,
            generation,
        } => {
            let view = Box::new(origin::repo_view(bin, &full_name));
            let api = Box::new(origin::get_repo(bin, &full_name));
            Msg::Detail {
                generation,
                full_name,
                view,
                api,
            }
        }
        Job::Branches {
            full_name,
            generation,
        } => {
            let result = origin::list_branches(bin, &full_name);
            Msg::Branches {
                generation,
                full_name,
                result,
            }
        }
        Job::Commits {
            full_name,
            branch,
            generation,
        } => {
            let result = origin::list_commits(bin, &full_name, &branch);
            Msg::Commits {
                generation,
                full_name,
                branch,
                result,
            }
        }
        Job::Pulls {
            full_name,
            generation,
        } => {
            let result = origin::list_pulls(bin, &full_name);
            Msg::Pulls {
                generation,
                full_name,
                result,
            }
        }
        Job::Update {
            full_name,
            generation,
            patch,
        } => {
            let result = Box::new(origin::update_repo(bin, &full_name, &patch));
            Msg::Updated {
                generation,
                full_name,
                result,
            }
        }
        Job::Create {
            generation,
            namespace,
            name,
            branch,
        } => {
            let full_name = if namespace.is_empty() {
                name.clone()
            } else {
                format!("{namespace}/{name}")
            };
            Msg::Created {
                generation,
                full_name,
                result: origin::create_repo(bin, &namespace, &name, &branch),
            }
        }
    }
}
