"""A farm run's body for the coordinator's `PUT /v1/admin/run/<name>` from what `GET` returned (docs/ENGINE.md 245).

GET annotates each of a run's alternates with read-only notes - `problems` (rev 11), `variant_notes` (rev 20),
`gpu_notes` (rev 22), `opt_in_notes` (rev 27) - and PUT refuses any key of an alternate it does not take as an unknown
field (zero-farm's coordinator, admin.ts: an alternate is `build_id`, `runtime`, `when`, `model`). So a body sent back
keeps exactly those keys of each alternate, whatever notes later revisions add.

    from mtgz.az.farm_admin import put_body
    body = put_body(client.call('GET', f'/v1/admin/run/{name}'), state='paused')
    client.call('PUT', f'/v1/admin/run/{name}', body=body)
"""

RUN_ALTERNATE_KEYS = ('build_id', 'runtime', 'when', 'model')


def put_body(got, **changes):
    """The body of a GET of a farm run, with `changes` applied and each alternate cut to the keys PUT takes."""
    body = dict(got)
    body.update(changes)
    if body.get('alternates'):
        body['alternates'] = [{k: v for k, v in a.items() if k in RUN_ALTERNATE_KEYS} for a in body['alternates']]
    return body
