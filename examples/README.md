# Erno Examples

This directory contains examples demonstrating how to use the Erno framework.

## Simple API Example

A basic REST API example showing common patterns for building APIs with Erno.

### Features Demonstrated

- **Route definitions** with path parameters
- **Multiple HTTP methods** (GET, POST, PUT, DELETE) on the same route
- **JSON serialization/deserialization**
- **Health check endpoints**
- **Application configuration**

### Running the Example

```bash
# View all available routes
cargo run --example simple_api -- routes

# Start the server
cargo run --example simple_api -- serve
# or simply:
cargo run --example simple_api

# View version information
cargo run --example simple_api -- version

# See all available commands
cargo run --example simple_api -- --help
```

### Routes

The example defines the following routes:

- `GET /api/health` - Health check
- `GET /api/users` - List all users
- `POST /api/users` - Create a new user
- `GET /api/users/{id}` - Get a specific user
- `PUT /api/users/{id}` - Update a user
- `DELETE /api/users/{id}` - Delete a user
- `GET /api/posts` - List all posts
- `GET /api/posts/{id}` - Get a specific post

Plus the framework's built-in routes:
- `GET /liveness` - Liveness probe
- `GET /readiness` - Readiness probe
- `GET /ws` - WebSocket endpoint

### Testing the API

Once the server is running (default port 3000), you can test the endpoints:

```bash
# List users
curl http://localhost:3000/api/users

# Get a specific user
curl http://localhost:3000/api/users/1

# Create a user
curl -X POST http://localhost:3000/api/users \
  -H "Content-Type: application/json" \
  -d '{"name": "John Doe", "email": "john@example.com"}'

# List posts
curl http://localhost:3000/api/posts
```

### Configuration

The example uses configuration files in the `config/` directory:
- `config/development.toml` - Development environment settings
- `config/test.toml` - Test environment settings

You can specify the environment using the `APP_ENVIRONMENT` variable:

```bash
APP_ENVIRONMENT=development cargo run --example simple_api
```
