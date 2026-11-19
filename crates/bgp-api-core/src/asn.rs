use std::borrow::Cow;

use mnmf_store::Db;

use crate::dto::{autnum_object, route_object};
use crate::model::{DatasetMetadata, ResponseMeta};
use crate::resource_model::{AsnResponse, RoutePage};
use crate::resources::{Page, paginate_routes};
use crate::service::ApiError;

pub(crate) fn lookup_asn<'a>(
    db: &'a Db,
    meta: &'a DatasetMetadata,
    asn: u32,
    page: Page,
) -> Result<Option<AsnResponse<'a>>, ApiError> {
    let autnum = db.autnum_ref(asn, false)?.map(autnum_object);
    let (total, ids) =
        db.asn_route_slice(asn, page.numbered, page.number, page.cursor, page.limit)?;
    if autnum.is_none() && total == 0 {
        return Ok(None);
    }
    let mut routes = Vec::with_capacity(ids.len());
    for id in ids {
        routes.push(route_object(id, db.route_ref(id, false)?));
    }
    let route_page = if page.numbered {
        let total_pages =
            usize::try_from((total + page.limit as i64 - 1) / page.limit as i64).unwrap_or(0);
        RoutePage {
            items: routes,
            next_cursor: None,
            page: Some(page.number),
            total_pages: Some(total_pages),
            total_items: Some(total),
        }
    } else {
        let (items, next_cursor) = paginate_routes(routes, page.limit);
        RoutePage {
            items,
            next_cursor,
            page: None,
            total_pages: None,
            total_items: None,
        }
    };
    Ok(Some(AsnResponse {
        meta: Some(ResponseMeta {
            dataset: Some(Cow::Borrowed(meta)),
        }),
        asn: format!("AS{asn}"),
        as_number: asn,
        autnum,
        routes: route_page,
    }))
}
