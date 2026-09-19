#pragma once

namespace kiyosi::detail {

#if defined(__CUDACC__)
#define KIYOSI_HOST_DEVICE __host__ __device__
#else
#define KIYOSI_HOST_DEVICE
#endif

enum class AutocallableTerminalKind : unsigned char {
    fixed,
    downside_if_knocked_in,
};

struct AutocallableProgram {
    double principal_ratio;
    double initial_price;
    double upper_strike;
    double lower_strike;
    double knock_in_price;
    double intact_terminal_coupon;
    double knocked_in_terminal_coupon;
    AutocallableTerminalKind terminal_kind;
    bool has_knock_in;
    bool daily_knock_in;
    bool carries_observation_coupon;
};

struct AutocallableEvent {
    double knock_out_price;
    double coupon;
    double coupon_barrier;
    bool conditional_coupon;
    bool active;
};

struct AutocallablePathState {
    double coupons;
    bool knocked_in;
};

KIYOSI_HOST_DEVICE inline bool program_knocked_in(
    const AutocallableProgram& program, double spot, bool knocked_in, bool expiry)
{
    if (program.has_knock_in && (program.daily_knock_in || expiry))
        return knocked_in || spot < program.knock_in_price;
    return knocked_in;
}

KIYOSI_HOST_DEVICE inline double program_observation_coupon(
    const AutocallableEvent& event, double spot)
{
    return event.conditional_coupon && spot < event.coupon_barrier ? 0.0 : event.coupon;
}

KIYOSI_HOST_DEVICE inline double program_terminal_settlement(
    const AutocallableProgram& program, double spot, bool knocked_in)
{
    if (knocked_in &&
        program.terminal_kind == AutocallableTerminalKind::downside_if_knocked_in) {
        const double bounded = spot < program.lower_strike
                                   ? program.lower_strike
                                   : (spot > program.upper_strike ? program.upper_strike : spot);
        return program.principal_ratio +
               (bounded - program.upper_strike) / program.initial_price;
    }
    return program.principal_ratio +
           (knocked_in ? program.knocked_in_terminal_coupon
                       : program.intact_terminal_coupon);
}

#undef KIYOSI_HOST_DEVICE

} // namespace kiyosi::detail
