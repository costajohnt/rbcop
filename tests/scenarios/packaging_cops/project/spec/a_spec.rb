require "bundler/setup"
require 'bundler/setup' if true
::Kernel.require "bundler/setup"
require "../lib/foo"
require "../lib"
require "./foo"
require_relative "../lib/foo/bar"
require_relative "helper"
require File.expand_path("../../lib/foo", __FILE__)
require File.expand_path("../../lib/foo", __dir__)
require File.expand_path("../lib/foo", __dir__)
require ::File.expand_path("../lib/foo", __dir__)
require File.expand_path("../../lib/foo", "x")
require File.dirname(__FILE__) + "/../lib/baz"
require File.dirname(__dir__) + "/../lib/baz2"
require "#{File.dirname(__FILE__)}/../lib/qux"
require "#{File.dirname(__FILE__)}/../lib/#{x}"
require "../lib/foo/lib/bar"
require("../lib/paren")
require "../lib/a", "b"
require "../libfoo/x"
foo&.require "../lib/csend"
