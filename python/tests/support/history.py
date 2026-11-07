"""Date historical fixtures at the valuation each scenario describes."""


def dated(instrument, context):
    if hasattr(instrument, "history_time") and instrument.history_time is None:
        return instrument.replace(history_time=context.valuation_time)
    return instrument
