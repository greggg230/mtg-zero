"""Shared assertion for the five EoE Planet lands (CR 702.184 Station).

``PLANET_LANDS`` (static registry) is the simplified legacy printing:
mana + enters-tapped only. The parser's printing is the faithful one: it
additionally carries ``station_threshold=12`` and the "12+ |" activated
ability (``tier_threshold=12``). Everything else must be identical.
"""

from dataclasses import replace


def assert_planet_matches_static(parsed, static) -> None:
    assert parsed.station_threshold == 12, parsed.name
    tiered = [a for a in parsed.activated_abilities if a.tier_threshold == 12]
    assert len(tiered) == 1, parsed.name
    assert len(parsed.activated_abilities) == 1, parsed.name
    assert replace(
        parsed,
        station_threshold=static.station_threshold,
        activated_abilities=static.activated_abilities,
    ) == static, f"Parity failure for {parsed.name!r}"
