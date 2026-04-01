#!/usr/bin/env node

// Debug test for calculator extension
const fs = require('fs');
const path = require('path');

// Read the calculator extension
const calculatorCode = fs.readFileSync(path.join(__dirname, 'extensions/calculator.js'), 'utf8');

console.log('=== Calculator Extension Debug ===\n');

// Mock the globalThis.raycast object
const mockResults = [];
global.globalThis = {
  raycast: {
    updateList: function(results) {
      console.log('updateList called with:', JSON.stringify(results, null, 2));
      mockResults.push(...results);
    }
  }
};

// Set globalThis globally
global.globalThis = global;

// Execute the calculator code
try {
  eval(calculatorCode);
  console.log('✓ Extension code executed successfully');
} catch (error) {
  console.log('✗ Extension code failed:', error.message);
}

// Check if onSearch is defined
if (typeof globalThis.onSearch === 'function') {
  console.log('✓ onSearch function is defined');
} else {
  console.log('✗ onSearch function is NOT defined');
}

console.log('\n=== Testing onSearch Function ===\n');

// Test simple calculation
try {
  console.log('Testing: onSearch("2 + 2")');
  globalThis.onSearch('2 + 2');
} catch (error) {
  console.log('Error:', error.message);
}

console.log('\n=== Test Complete ===');
console.log(`Total results generated: ${mockResults.length}`);