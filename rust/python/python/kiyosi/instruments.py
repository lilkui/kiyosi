"""Validated European vanilla contracts."""

from typing import Literal

from ._native import EuropeanOption

OptionType = Literal["call", "put"]

__all__ = ["EuropeanOption", "OptionType"]
