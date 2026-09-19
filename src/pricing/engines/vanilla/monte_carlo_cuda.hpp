#pragma once

#include <cstdint>

namespace kiyosi::detail {

enum class CudaPricingStatus : unsigned char {
    success,
    unavailable,
    failure,
    out_of_memory,
    invalid_result,
};

struct CudaEuropeanRequest {
    int path_count;
    int step_count;
    std::uint64_t seed;
    double spot;
    double strike;
    double drift;
    double diffusion;
    int payoff_sign;
};

struct CudaAmericanRequest {
    int path_count;
    int step_count;
    std::uint64_t seed;
    double spot;
    double strike;
    double drift;
    double diffusion;
    double discount;
    int payoff_sign;
};

struct CudaPricingResult {
    CudaPricingStatus status;
    double payoff_sum;
    const char* message;
};

[[nodiscard]] CudaPricingResult cuda_european_price(CudaEuropeanRequest request);
[[nodiscard]] CudaPricingResult cuda_american_price(CudaAmericanRequest request);

} // namespace kiyosi::detail
