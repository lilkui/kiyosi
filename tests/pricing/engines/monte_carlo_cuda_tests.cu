#include <catch2/catch_approx.hpp>
#include <catch2/catch_test_macros.hpp>
#include <cuda_runtime.h>

#include <algorithm>
#include <array>
#include <limits>
#include <memory>
#include <vector>

#include "pricing/engines/monte_carlo_cuda_reduction.hpp"
#include "pricing/engines/vanilla/monte_carlo_regression.hpp"

namespace {
struct DeviceDeleter {
    void operator()(void* pointer) const { cudaFree(pointer); }
};

__global__ void solve_regression(const double* input, double* output)
{
    double samples[3][4];     // NOLINT(modernize-avoid-c-arrays): exercise the shared solver with device-accessible storage.
    double coefficients[3]{}; // NOLINT(modernize-avoid-c-arrays): device code cannot call host std::array methods.
    for (int row = 0; row < 3; ++row)
        for (int column = 0; column < 4; ++column)
            samples[row][column] = input[row * 4 + column];
    output[3] = kiyosi::detail::solve_quadratic(samples, coefficients) ? 1.0 : 0.0;
    for (int index = 0; index < 3; ++index)
        output[index] = coefficients[index];
}
} // namespace

TEST_CASE("CUDA regression solves quadratic and reduced bases", "[cuda]")
{
    void* input_pointer = nullptr;
    REQUIRE(cudaMalloc(&input_pointer, 12 * sizeof(double)) == cudaSuccess);
    const std::unique_ptr<void, DeviceDeleter> input{input_pointer};
    void* output_pointer = nullptr;
    REQUIRE(cudaMalloc(&output_pointer, 4 * sizeof(double)) == cudaSuccess);
    const std::unique_ptr<void, DeviceDeleter> output{output_pointer};
    const auto check = [&](const std::array<double, 12>& samples, const std::array<double, 3>& expected, bool valid) {
        REQUIRE(cudaMemcpy(input.get(), samples.data(), sizeof(samples), cudaMemcpyHostToDevice) == cudaSuccess);
        solve_regression<<<1, 1>>>(static_cast<const double*>(input.get()), static_cast<double*>(output.get()));
        REQUIRE(cudaGetLastError() == cudaSuccess);
        std::array<double, 4> actual{};
        REQUIRE(cudaMemcpy(actual.data(), output.get(), sizeof(actual), cudaMemcpyDeviceToHost) == cudaSuccess);
        CHECK(actual[3] == (valid ? 1.0 : 0.0));
        if (valid)
            for (int index = 0; index < 3; ++index)
                CHECK(actual[index] == Catch::Approx(expected[index]).margin(1e-12));
    };
    check({3, 0, 2, 14, 0, 2, 0, 6, 2, 0, 2, 12}, {2, 3, 4}, true);
    check({2, 1, 1, 7, 1, 1, 1, 5, 1, 1, 1, 5}, {2, 3, 0}, true);
    check({4, 4, 4, 20, 4, 4, 4, 20, 4, 4, 4, 20}, {5, 0, 0}, true);
    check({}, {}, false);
    std::array<double, 12> non_finite{};
    non_finite[0] = std::numeric_limits<double>::quiet_NaN();
    check(non_finite, {}, false);
}

TEST_CASE("CUDA payoff reduction preserves weights and extreme values across blocks", "[cuda][monte-carlo-performance]")
{
    using namespace kiyosi::detail;
    const auto allocate = [](std::size_t bytes) {
        void* pointer = nullptr;
        REQUIRE(cudaMalloc(&pointer, bytes) == cudaSuccess);
        return std::unique_ptr<void, DeviceDeleter>{pointer};
    };
    const auto input = allocate(100'003 * sizeof(double));
    const auto partials = allocate(256 * sizeof(MonteCarloMean));
    const auto output = allocate(sizeof(double));
    const auto reduce = [&](const std::vector<double>& values) {
        const int count = static_cast<int>(values.size());
        const int blocks = std::min((count + monte_carlo_reduction_threads - 1) / monte_carlo_reduction_threads, 256);
        REQUIRE(cudaMemcpy(input.get(), values.data(), values.size() * sizeof(double), cudaMemcpyHostToDevice) == cudaSuccess);
        reduce_payoffs<<<blocks, monte_carlo_reduction_threads>>>(static_cast<const double*>(input.get()), count,
                                                                  static_cast<MonteCarloMean*>(partials.get()));
        REQUIRE(cudaGetLastError() == cudaSuccess);
        reduce_payoffs<<<1, monte_carlo_reduction_threads>>>(static_cast<const MonteCarloMean*>(partials.get()), blocks,
                                                             static_cast<double*>(output.get()));
        REQUIRE(cudaGetLastError() == cudaSuccess);
        double result = 0.0;
        REQUIRE(cudaMemcpy(&result, output.get(), sizeof(double), cudaMemcpyDeviceToHost) == cudaSuccess);
        return result;
    };
    for (const int count : {1, 255, 256, 257, 65'535, 65'536, 65'537, 100'003}) {
        CAPTURE(count);
        std::vector<double> values(static_cast<std::size_t>(count));
        double sum = 0.0;
        for (int index = 0; index < count; ++index) {
            values[static_cast<std::size_t>(index)] = index % 13 - 6.0;
            sum += values[static_cast<std::size_t>(index)];
        }
        CHECK(reduce(values) == Catch::Approx(sum / count).margin(1e-14));
    }
    const double maximum = std::numeric_limits<double>::max();
    for (const double value : {maximum, -maximum, std::numeric_limits<double>::denorm_min()}) {
        CAPTURE(value);
        CHECK(reduce(std::vector<double>(100'003, value)) == value);
    }
    std::vector<double> cancellation(100'002);
    for (std::size_t index = 0; index < cancellation.size(); ++index)
        cancellation[index] = index % 2 == 0 ? maximum : -maximum;
    CHECK(reduce(cancellation) == 0.0);
}
