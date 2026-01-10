# Testing Guide

This document describes the test suite for the Chemins Noirs backend.

## Test Categories

### 1. Unit Tests (46 tests)
Fast, focused tests for individual functions and modules.

```bash
# Run all unit tests (excluding proptests and database tests)
cargo test --lib -- --skip proptests --skip database

# Run specific module tests
cargo test elevation::tests --lib
cargo test loops::tests --lib
cargo test routing::tests --lib
```

**Coverage:**
- `elevation.rs`: 17 tests (median, haversine, smoothing, profile creation)
- `loops.rs`: 9 tests (geographic calculations, destination points)
- `routing.rs`: 4 tests (haversine, distance approximation)
- `engine.rs`: 7 tests (pathfinding, graph operations)
- `graph.rs`: 8 tests (edge building, intersection detection)
- `terrain.rs`: 1 test (mesh building)

---

### 2. Integration Tests (10 tests)
Database integration tests using testcontainers with PostgreSQL.

**Prerequisites:**
- Docker must be installed and running
- Testcontainers will automatically pull `postgres:17-alpine`

```bash
# Run database integration tests
cargo test database::tests --lib

# Run specific database test
cargo test database::tests::test_save_and_retrieve_route --lib
```

**Coverage:**
- Connection pooling
- CRUD operations (save, retrieve, list, delete)
- Favorite toggling
- Error handling (NotFound, etc.)
- Optional field handling

**Execution time:** ~20 seconds (includes container startup)

---

### 3. Property-Based Tests (17 tests)
Randomized testing with proptest to verify mathematical properties.

**Default configuration:** 256 test cases per property

```bash
# Run all property tests (may take several minutes)
cargo test proptests --lib

# Run with reduced cases for faster feedback (recommended for development)
PROPTEST_CASES=10 cargo test proptests --lib

# Run specific property test
PROPTEST_CASES=100 cargo test prop_haversine_symmetric --lib
```

**Coverage:**

**loops.rs proptests (7 tests):**
- Normalization invariants (longitude, bearing)
- Geographic calculation bounds
- Mathematical properties (idempotence, modular arithmetic)

**routing.rs proptests (10 tests):**
- Distance metric properties (non-negativity, symmetry, triangle inequality)
- Vector operations (perpendicularity, normalization)
- Distance additivity

**Execution time:**
- 10 cases: ~5 seconds
- 100 cases: ~30 seconds
- 256 cases (default): ~2 minutes

---

## Running All Tests

```bash
# Quick test run (unit tests only)
cargo test --lib -- --skip proptests --skip database

# Full test suite (requires Docker)
cargo test --lib

# CI mode (with coverage)
cargo tarpaulin --workspace --timeout 600 --out Xml
```

---

## Test Configuration

### Environment Variables

- `DATABASE_URL`: PostgreSQL connection string for integration tests
  ```bash
  export DATABASE_URL="postgres://postgres:postgres@localhost:5432/postgres"
  ```

- `PROPTEST_CASES`: Number of random test cases for property tests
  ```bash
  export PROPTEST_CASES=100
  ```

### Cargo.toml Test Dependencies

```toml
[dev-dependencies]
testcontainers = "0.23"
testcontainers-modules = { version = "0.11", features = ["postgres"] }
proptest = "1.6"
criterion = { version = "0.5", features = ["html_reports"] }
```

---

## Continuous Integration

Tests run automatically on GitHub Actions for all PRs and pushes to master:

- **Unit tests**: All platforms
- **Integration tests**: With PostgreSQL service container
- **Property tests**: Full 256 cases (cached)
- **Coverage**: Tracked via Codecov

See `.github/workflows/ci.yml` for details.

---

## Benchmarks

Performance benchmarks are available for graph generation:

```bash
cargo bench --bench graph_generation
```

Results are saved to `target/criterion/` with HTML reports.

---

## Test Best Practices

1. **Write unit tests first** for new functions
2. **Use property tests** for mathematical invariants
3. **Add integration tests** for database operations
4. **Run quick tests locally** (`--skip proptests --skip database`)
5. **Let CI run full suite** before merging

---

## Troubleshooting

### Docker Issues
```bash
# Verify Docker is running
docker ps

# Clean up stopped containers
docker container prune
```

### Property Test Failures
```bash
# Reduce test cases to isolate issue
PROPTEST_CASES=1 cargo test prop_failing_test --lib -- --nocapture

# Check proptest-regressions/ for failing cases
cat proptest-regressions/*.txt
```

### Database Connection Errors
```bash
# Verify PostgreSQL is accessible
psql $DATABASE_URL -c "SELECT 1"

# Check testcontainers logs
docker logs <container_id>
```

---

## Test Metrics

| Metric | Value |
|--------|-------|
| Total tests | 73+ |
| Unit tests | 46 |
| Integration tests | 10 |
| Property tests | 17 |
| Code coverage | ~45% |
| Test execution time | ~25s (full suite) |
