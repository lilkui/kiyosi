#pragma once

#include <cstddef>
#include <cstdint>
#include <span>

#include "../detail/autocallable_program.hpp"

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

struct CudaSimulationStep {
    double drift;
    double diffusion;
    double discount;
};

struct CudaStructuredStep {
    CudaSimulationStep simulation;
    AutocallableEvent event;
};

struct CudaAccumulatorRequest {
    int path_count;
    std::uint64_t seed;
    double spot;
    double strike;
    double knock_out;
    double daily_quantity;
    double acceleration;
    double initial_quantity;
};

struct CudaStructuredRequest {
    int path_count;
    std::uint64_t seed;
    double spot;
    double terminal_discount;
    AutocallableProgram program;
    AutocallablePathState initial_state;
};

struct CudaPricingResult {
    CudaPricingStatus status;
    double payoff_sum;
    const char* message;
};

[[nodiscard]] CudaPricingResult cuda_european_price(CudaEuropeanRequest request);
[[nodiscard]] CudaPricingResult cuda_american_price(CudaAmericanRequest request);
[[nodiscard]] CudaPricingResult cuda_accumulator_price(
    CudaAccumulatorRequest request, std::span<const CudaSimulationStep> steps);
[[nodiscard]] CudaPricingResult cuda_structured_price(
    CudaStructuredRequest request, std::span<const CudaStructuredStep> steps);

} // namespace kiyosi::detail
