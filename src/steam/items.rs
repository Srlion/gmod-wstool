use std::sync::mpsc;
use steamworks::{
    AppIDs, PublishedFileVisibility, UGCStatisticType, UGCType, UserList, UserListOrder,
};

use crate::steam::update;

use super::client;

pub struct WorkshopItem {
    pub id: u64,
    pub title: String,
    pub description: String,
    pub tags: Vec<String>,
    pub file_size: u32,
    pub preview_url: Option<String>,
    pub num_upvotes: u32,
    pub num_downvotes: u32,
    pub subscriptions: Option<u64>,
    pub visibility: u8,
}

pub struct WorkshopQueryResult {
    pub items: Vec<WorkshopItem>,
    pub page: u32,
    pub total_results: u32,
    pub total_pages: u32,
}

pub fn query_items(page: u32) -> Result<WorkshopQueryResult, String> {
    let (tx, rx) = mpsc::channel::<Result<WorkshopQueryResult, String>>();

    let client = client();
    let user = client.user();
    let account = user.steam_id().account_id();
    let ugc = client.ugc();

    let query = ugc
        .query_user(
            account,
            UserList::Published,
            UGCType::ItemsReadyToUse,
            UserListOrder::LastUpdatedDesc,
            AppIDs::ConsumerAppId(steamworks::AppId(4000)),
            page,
        )
        .unwrap()
        .set_return_long_description(true);

    query.fetch(move |res| {
        let result: Result<WorkshopQueryResult, String> = match res {
            Ok(results) => {
                let mut items = Vec::new();

                for i in 0..results.returned_results() {
                    if let Some(r) = results.get(i) {
                        let preview_url = results.preview_url(i);

                        items.push(WorkshopItem {
                            id: r.published_file_id.0,
                            title: r.title,
                            description: r.description,
                            tags: r.tags,
                            file_size: r.file_size,
                            preview_url,
                            num_upvotes: r.num_upvotes,
                            num_downvotes: r.num_downvotes,
                            subscriptions: results.statistic(i, UGCStatisticType::Subscriptions),
                            visibility: match r.visibility {
                                PublishedFileVisibility::Public => update::VIS_PUBLIC,
                                PublishedFileVisibility::FriendsOnly => update::VIS_FRIENDS,
                                PublishedFileVisibility::Private => update::VIS_PRIVATE,
                                PublishedFileVisibility::Unlisted => update::VIS_UNLISTED,
                            },
                        });
                    }
                }

                let total = results.total_results();
                let per_page = results.returned_results().max(1);
                let total_pages = total.div_ceil(per_page);

                Ok(WorkshopQueryResult {
                    items,
                    page,
                    total_results: total,
                    total_pages: total_pages.max(1),
                })
            }
            Err(e) => Err(e.to_string()),
        };

        let _ = tx.send(result);
    });

    rx.recv()
        .map_err(|_| "steamworks query was cancelled".to_string())?
}

pub fn fetch_title(id: u64) -> Option<String> {
    let (tx, rx) = mpsc::channel::<Option<String>>();
    let client = client();
    let ugc = client.ugc();

    ugc.query_items(vec![steamworks::PublishedFileId(id)])
        .unwrap()
        .fetch(move |res| {
            let title = res
                .ok()
                .and_then(|r| r.get(0))
                .map(|item| item.title.clone());
            let _ = tx.send(title);
        });

    rx.recv().ok().flatten()
}
