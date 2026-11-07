"""Date historical fixtures at the valuation each scenario describes."""


def dated(instrument, context):
    if (
        hasattr(instrument, "history_time")
        and instrument.history_time is None
        and instrument.effective_time
        <= context.valuation_time
        <= instrument.expiry_time
    ):
        return instrument.replace(history_time=context.valuation_time)
    return instrument
