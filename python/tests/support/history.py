"""Date historical fixtures at the valuation each scenario describes."""

from datetime import timedelta


def dated(instrument, context):
    if (
        hasattr(instrument, "history_time")
        and instrument.effective_time
        <= context.valuation_time
        <= instrument.expiry_time
    ):
        if (
            getattr(instrument, "realized_average", None) is not None
            and context.valuation_time <= instrument.averaging_start_time
        ):
            return instrument
        payloads = {
            name: getattr(instrument, name)
            for name in (
                "touch_state",
                "barrier_state",
                "realized_average",
                "realized_hit_count",
                "accumulated_quantity",
                "knock_out_settled",
            )
            if hasattr(instrument, name)
        }
        return instrument.replace(**payloads, history_time=context.valuation_time)
    return instrument


def history_fields(**fields):
    """Date initial historical fixtures; pricing scenarios set their own anchor."""
    if "history_time" not in fields and any(
        fields.get(name) is not None
        for name in (
            "touch_state",
            "barrier_state",
            "realized_average",
            "realized_hit_count",
            "accumulated_quantity",
            "knock_out_settled",
        )
    ):
        fields["history_time"] = (
            fields["averaging_start_time"] + timedelta(microseconds=1)
            if fields.get("realized_average") is not None
            else fields["effective_time"]
        )
    return fields
