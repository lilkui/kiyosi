#pragma once

#if defined(__CUDACC__)
#define KIYOSI_ACCUMULATOR_HOST_DEVICE __host__ __device__
#else
#define KIYOSI_ACCUMULATOR_HOST_DEVICE
#endif

namespace kiyosi::detail {

KIYOSI_ACCUMULATOR_HOST_DEVICE inline bool accumulator_knocked_out(double spot, double knock_out_level)
{
    return spot >= knock_out_level;
}

// Call only for observations that have not knocked out.
KIYOSI_ACCUMULATOR_HOST_DEVICE inline double accumulator_purchase_quantity(
    double spot, double strike, double daily_quantity, double acceleration_factor)
{
    return spot < strike ? daily_quantity * acceleration_factor : daily_quantity;
}

} // namespace kiyosi::detail

#undef KIYOSI_ACCUMULATOR_HOST_DEVICE
