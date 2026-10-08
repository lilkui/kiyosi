#pragma once

#include <cmath>

#if defined(__CUDACC__)
#define KIYOSI_HOST_DEVICE __host__ __device__
#else
#define KIYOSI_HOST_DEVICE
#endif

namespace kiyosi::detail {

// A power-of-two scale protects totals from overflow without discarding subnormal payoffs.
struct MonteCarloMean {
    double sum;
    double correction;
    int exponent;
    int count;

    KIYOSI_HOST_DEVICE void add(double sample)
    {
        int sample_exponent = 0;
        const double fraction = std::frexp(sample, &sample_exponent);
        merge({fraction, 0.0, sample_exponent, 1});
    }

    KIYOSI_HOST_DEVICE void merge(const MonteCarloMean& other)
    {
        if (other.count == 0) return;
        if (sum == 0.0) {
            sum = other.sum;
            correction = other.correction;
            exponent = other.exponent;
        } else if (other.sum != 0.0) {
            if (other.exponent > exponent) {
                sum = std::ldexp(sum, exponent - other.exponent);
                correction = std::ldexp(correction, exponent - other.exponent);
                exponent = other.exponent;
            }
            const double adjusted = std::ldexp(other.sum, other.exponent - exponent) -
                                    std::ldexp(other.correction, other.exponent - exponent) - correction;
            const double next = sum + adjusted;
            correction = (next - sum) - adjusted;
            sum = next;
        }
        count += other.count;
    }

    KIYOSI_HOST_DEVICE double value() const
    {
        return std::ldexp(sum / static_cast<double>(count), exponent);
    }
};

} // namespace kiyosi::detail

#undef KIYOSI_HOST_DEVICE
