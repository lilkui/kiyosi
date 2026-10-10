#pragma once

#include <cstddef>
#include <cstdint>
#include <span>

#include <kiyosi/pricing/detail/autocallable_program.hpp>

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

struct CudaAutocallableStep {
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

struct CudaAutocallableRequest {
    int path_count;
    std::uint64_t seed;
    double spot;
    double terminal_discount;
    AutocallableProgram program;
    AutocallablePathState initial_state;
};

struct CudaPricingResult {
    CudaPricingStatus status;
    double payoff_mean;
    const char* message;
};

[[nodiscard]] CudaPricingResult cuda_european_price(CudaEuropeanRequest request);
[[nodiscard]] CudaPricingResult cuda_american_price(CudaAmericanRequest request);
[[nodiscard]] CudaPricingResult cuda_accumulator_price(
    CudaAccumulatorRequest request, std::span<const CudaSimulationStep> steps);
[[nodiscard]] CudaPricingResult cuda_autocallable_price(
    CudaAutocallableRequest request, std::span<const CudaAutocallableStep> steps);

} // namespace kiyosi::detail
