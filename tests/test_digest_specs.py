"""The specs the behaviour digests beyond the cube table play (engine/tests/digest_formats.rs, docs/ENGINE.md 148) are
the presets' specs: engine/tests/data/digest_specs.json holds copies of them. A preset change that alters one fails
here: copy the new spec into the file and re-record that digest (DIGEST_WRITE=1), stating old -> new."""
import json
import os

import mtgz.az as A

SPECS = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'engine', 'tests', 'data',
                     'digest_specs.json')


def test_the_digest_specs_are_the_presets_specs():
    with open(SPECS, encoding='utf-8') as f:
        specs = json.load(f)
    assert specs['delver_bo3'] == json.loads(A.SPEC_DELVER_BO3)
    assert specs['delver_bo3'] == json.loads(A.PRESETS['delver-mirror-bo3']['spec'])
    assert specs['eoe'] == json.loads(A.SPEC_EOE) == json.loads(A.PRESETS['eoe-1']['spec'])
    assert specs['eoe_choice'] == json.loads(A.SPEC_EOE_CHOICE) == json.loads(A.PRESETS['eoe-2']['spec'])
    # (the three-deck Legacy metagame, docs/ENGINE.md 176: metagame_digests)
    assert specs['meta3'] == json.loads(A.SPEC_META3) == json.loads(A.PRESETS['legacy-meta3']['spec'])
    assert set(specs) == {'_doc', 'delver_bo3', 'eoe', 'eoe_choice', 'meta3'}
