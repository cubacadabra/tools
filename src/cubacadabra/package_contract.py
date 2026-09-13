"""Shared validation for identifiers used by portable game packages."""

from __future__ import annotations

import re


MIN_GAME_ID_LENGTH = 3
MAX_GAME_ID_LENGTH = 64
GAME_ID_RE = re.compile(r"^[a-z0-9]+(?:-[a-z0-9]+)*$")


def is_valid_game_id(value: object) -> bool:
    return (
        isinstance(value, str)
        and MIN_GAME_ID_LENGTH <= len(value) <= MAX_GAME_ID_LENGTH
        and GAME_ID_RE.fullmatch(value) is not None
    )
