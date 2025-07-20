#!/bin/bash

echo "Broxy Logging Examples"
echo "======================"
echo ""

echo "1. Run with default log level (INFO):"
echo "   cargo run"
echo ""

echo "2. Run with DEBUG level:"
echo "   RUST_LOG=debug cargo run"
echo ""

echo "3. Run with TRACE level (most verbose):"
echo "   RUST_LOG=trace cargo run"
echo ""

echo "4. Run with specific module logging:"
echo "   RUST_LOG=broxy=debug,info cargo run"
echo ""

echo "5. Run with WARN level only:"
echo "   RUST_LOG=warn cargo run"
echo ""

echo "6. Run with ERROR level only:"
echo "   RUST_LOG=error cargo run"
echo ""

echo "Available log levels: error, warn, info, debug, trace"
echo ""
echo "You can also set specific module levels:"
echo "  RUST_LOG=broxy=debug,hyper=info,tokio=warn cargo run" 